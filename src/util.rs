use std::{
    collections::VecDeque,
    fmt::Debug,
    io::{self, BufRead},
    path::Path,
    sync::{Arc, Condvar, Mutex},
    time::Instant,
};

use anyhow::Context;
use log::debug;

use crate::io::fs::FileSystem;

/// Iterates over the lines in a file and calls the callback with a &str reference to each line.
/// This function does not allocate new strings for each line, as opposed to using
/// [`io::BufReader::lines()`] as in [`read_lines`].
pub fn read_lines_no_alloc<P>(
    fs: &impl FileSystem,
    filename: P,
    mut line_callback: impl FnMut(&str),
) -> io::Result<()>
where
    P: AsRef<Path> + Debug,
{
    debug!("Reading lines from {filename:?}");
    let start = Instant::now();

    let mut reader = fs.open(filename)?;

    let mut line_buffer = String::new();
    let mut line_count: u32 = 0;
    let mut byte_count: usize = 0;
    loop {
        let bytes_read = reader.read_line(&mut line_buffer)?;

        if bytes_read == 0 {
            break;
        }

        line_count += 1;
        byte_count += bytes_read;

        // the read line contains the newline delimiter, so we need to trim it off
        let line = line_buffer.trim_end();
        line_callback(line);
        line_buffer.clear();
    }

    let elapsed = start.elapsed();
    if line_count == 0 {
        debug!("No lines read");
        return Ok(());
    }
    debug!(
        "Read {} lines in {:.2?} ({:.2?}/line), total {} bytes ({:.2} bytes/second, {:?}/byte, {:.2} bytes/line)",
        line_count,
        elapsed,
        elapsed / line_count,
        byte_count,
        byte_count as f64 / elapsed.as_secs_f64(),
        elapsed / byte_count as u32,
        byte_count as f64 / line_count as f64,
    );

    Ok(())
}

/// Helper struct to time operations. Keeps track of the total time taken until the object is
/// dropped, as well as timing between individual sub-sections of the operation.
/// Timing information is printed using debug level log messages.
pub struct Timing {
    name: &'static str,
    start: Instant,
    current_section: Option<TimingSection>,
}

struct TimingSection {
    name: &'static str,
    start: Instant,
}

impl Timing {
    /// Start a new timing from now.
    pub fn start_now(name: &'static str) -> Self {
        debug!("[timing: {name}] Starting timing");
        Self {
            name,
            start: Instant::now(),
            current_section: None,
        }
    }

    /// Start a new timing section. This will end any already existing sections.
    pub fn start_section(&mut self, name: &'static str) {
        let now = self.end_section().unwrap_or(Instant::now());

        debug!("[timing: {}] Entering section '{}'", self.name, name);

        self.current_section = Some(TimingSection { name, start: now })
    }

    /// Ends the currnently active section and returns its end time, or does nothing
    /// if no section is active and returns `None`.
    pub fn end_section(&mut self) -> Option<Instant> {
        if let Some(s) = self.current_section.take() {
            //
            let now = Instant::now();
            debug!(
                "[timing: {}] Leaving section '{}', which took {:.3?}",
                self.name,
                s.name,
                now - s.start
            );
            Some(now)
        } else {
            None
        }
    }
}

impl Drop for Timing {
    fn drop(&mut self) {
        self.end_section();

        debug!(
            "[timing: {}] Stopping timing. Total: {:.3?} elapsed.",
            self.name,
            self.start.elapsed()
        );
    }
}

/// Helper to read an object serialized to disk
pub fn read_object<R: std::io::Read, O: serde::de::DeserializeOwned>(
    mut reader: R,
) -> anyhow::Result<O> {
    let value: bincode_next::serde::Compat<O> =
        bincode_next::decode_from_std_read(&mut reader, bincode_next::config::standard())
            .context("deserializing from file")?;
    Ok(value.0)
}

/// Helper to write an object to disk
pub fn write_object<W: std::io::Write, O: serde::Serialize>(
    mut writer: W,
    value: &O,
) -> anyhow::Result<()> {
    bincode_next::encode_into_std_write(
        bincode_next::serde::Compat(value),
        &mut writer,
        bincode_next::config::standard(),
    )
    .context("serializing to file")?;
    Ok(())
}

/// A simple Single-Producer-Multiple-Consumer queue for passing work to processing threads.
pub fn make_queue<T>() -> (Producer<T>, Consumer<T>) {
    let inner = Inner {
        inner: Mutex::new(InnerMut {
            queue: VecDeque::new(),
            has_closed: false,
        }),
        var_has_items: Condvar::new(),
    };
    let inner = Arc::new(inner);

    let producer = Producer {
        inner: Arc::clone(&inner),
    };
    let consumer = Consumer { inner };
    (producer, consumer)
}

struct Inner<T> {
    inner: Mutex<InnerMut<T>>,
    var_has_items: Condvar,
}

struct InnerMut<T> {
    queue: VecDeque<T>,
    has_closed: bool,
}

pub struct Producer<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Producer<T> {
    /// Pushes an item to the queue. Returns `Err` if the queue has been closed.
    pub fn push(&self, item: T) {
        let mut inner = self.inner.inner.lock().unwrap();
        inner.queue.push_back(item);
        self.inner.var_has_items.notify_one();
    }
}

impl<T> Drop for Producer<T> {
    fn drop(&mut self) {
        let mut inner = self.inner.inner.lock().unwrap();
        inner.has_closed = true;
        self.inner.var_has_items.notify_all();
    }
}

#[derive(Clone)]
pub struct Consumer<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Consumer<T> {
    /// Pops an item from the queue. Returns `None` if the queue has been closed and is empty.
    pub fn pop(&self) -> Option<T> {
        let mut inner = self.inner.inner.lock().unwrap();
        loop {
            if let Some(item) = inner.queue.pop_front() {
                return Some(item);
            }
            if inner.has_closed {
                return None;
            }
            inner = self.inner.var_has_items.wait(inner).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_queue() {
        let (producer, consumer) = make_queue();

        let producer_thread = thread::spawn(move || {
            for i in 0..10 {
                producer.push(i);
            }
        });

        let consumer_thread = thread::spawn(move || {
            let mut items = Vec::new();
            while let Some(item) = consumer.pop() {
                items.push(item);
            }
            items
        });

        producer_thread.join().unwrap();
        let items = consumer_thread.join().unwrap();
        assert_eq!(items, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn test_multiple_consumer() {
        let (producer, consumer) = make_queue();

        let producer_thread = thread::spawn(move || {
            for i in 0..10 {
                producer.push(i);
            }
        });

        let consumer_thread1 = thread::spawn({
            let consumer = consumer.clone();
            move || {
                let mut items = Vec::new();
                while let Some(item) = consumer.pop() {
                    items.push(item);
                }
                items
            }
        });

        let consumer_thread2 = thread::spawn({
            let consumer = consumer.clone();
            move || {
                let mut items = Vec::new();
                while let Some(item) = consumer.pop() {
                    items.push(item);
                }
                items
            }
        });

        producer_thread.join().unwrap();
        let items1 = consumer_thread1.join().unwrap();
        let items2 = consumer_thread2.join().unwrap();

        assert_eq!(items1.len() + items2.len(), 10);
        assert_eq!(
            [items1, items2]
                .concat()
                .into_iter()
                .collect::<std::collections::HashSet<_>>(),
            (0..10).collect::<std::collections::HashSet<_>>()
        );
    }
}
