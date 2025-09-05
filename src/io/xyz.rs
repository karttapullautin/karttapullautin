use crate::io::bytes::FromToBytes;
use std::{
    io::{Read, Seek, Write},
    time::Instant,
};

use log::debug;

/// The magic number that identifies a valid XYZ binary file.
const XYZ_MAGIC: &[u8] = b"XYZB";

/// A single record of an observed laser data point needed by the algorithms.
#[derive(Debug, Clone, Copy, Default, PartialEq, bytemuck::NoUninit, bytemuck::AnyBitPattern)]
#[repr(C)]
pub struct XyzRecord {
    pub x: f64,
    pub y: f64,
    pub z: f32,
    pub classification: u8,
    pub number_of_returns: u8,
    pub return_number: u8,
    // padding bytes to make the struct exactly 24 bytes long
    pub _padding: u8,
}

pub struct XyzInternalWriter<W: Write + Seek> {
    inner: Option<W>,
    records_written: u64,
    // for stats
    start: Option<Instant>,
}

impl<W: Write + Seek> XyzInternalWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner: Some(inner),
            records_written: 0,
            start: None,
        }
    }

    pub fn write_records(&mut self, records: &[XyzRecord]) -> std::io::Result<()> {
        let inner = self
            .inner
            .as_mut()
            .ok_or_else(|| std::io::Error::other("writer has already been finished"))?;

        if records.is_empty() {
            return Ok(()); // nothing to write
        }

        // write the header (format + length) on the first write
        if self.records_written == 0 {
            self.start = Some(Instant::now());

            inner.write_all(XYZ_MAGIC)?;
            // Write the temporary number of records as all FF
            u64::MAX.to_bytes(inner)?;
        }

        let bytes: &[u8] = bytemuck::cast_slice(records);
        inner.write_all(bytes)?;

        self.records_written += records.len() as u64;
        Ok(())
    }

    pub fn finish(&mut self) -> std::io::Result<W> {
        let mut inner = self
            .inner
            .take()
            .ok_or_else(|| std::io::Error::other("writer has already been finished"))?;

        // seek to the beginning of the file and write the number of records
        inner.seek(std::io::SeekFrom::Start(XYZ_MAGIC.len() as u64))?;
        self.records_written.to_bytes(&mut inner)?;

        // log statistics about the written records
        if let Some(start) = self.start {
            let elapsed = start.elapsed();
            debug!(
                "Wrote {} records in {:.2?} ({:.2?}/record, {:.3}M records/s, {:.2}MB/s)",
                self.records_written,
                elapsed,
                elapsed / self.records_written as u32,
                self.records_written as f64 / (10e6 * elapsed.as_secs_f64()),
                self.records_written as f64 * size_of::<XyzRecord>() as f64
                    / (1024.0 * 1024.0 * elapsed.as_secs_f64()),
            );
        }
        Ok(inner)
    }
}

impl<W: Write + Seek> Drop for XyzInternalWriter<W> {
    fn drop(&mut self) {
        if self.inner.is_some() {
            self.finish().expect("failed to finish writer in Drop");
        }
    }
}

pub struct XyzInternalReader<R: Read> {
    inner: R,
    n_records: u64,
    records_read: u64,
    // for stats
    start: Option<Instant>,
    buffer: [XyzRecord; 1024],
}

impl<R: Read> XyzInternalReader<R> {
    pub fn new(mut inner: R) -> std::io::Result<Self> {
        // read and check the magic number
        let mut buff = [0; XYZ_MAGIC.len()];
        inner.read_exact(&mut buff)?;
        if buff != XYZ_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid magic number",
            ));
        }

        // read the number of records, defined by the first u64
        let n_records = u64::from_bytes(&mut inner)?;
        Ok(Self {
            inner,
            n_records,
            records_read: 0,
            start: None,
            buffer: [XyzRecord::default(); 1024],
        })
    }

    pub fn next_chunk(&mut self) -> std::io::Result<Option<&[XyzRecord]>> {
        if self.records_read >= self.n_records {
            // TODO: log statistics about the read records
            if let Some(start) = self.start {
                let elapsed = start.elapsed();
                debug!(
                    "Read {} records in {:.2?} ({:.2?}/record, {:.3}M records/s, {:.2}MB/s)",
                    self.records_read,
                    elapsed,
                    elapsed / self.records_read as u32,
                    self.records_read as f64 / (10e6 * elapsed.as_secs_f64()),
                    self.records_read as f64 * size_of::<XyzRecord>() as f64
                        / (1024.0 * 1024.0 * elapsed.as_secs_f64()),
                );
            }

            return Ok(None);
        }

        if self.records_read == 0 {
            self.start = Some(Instant::now());
        }

        // read as many as we can fit in the buffer
        let records_left = self.n_records - self.records_read;
        let records_to_read = (self.buffer.len() as u64).min(records_left);

        // treat buffer as mutable slice of bytes
        let records_buffer = &mut self.buffer[..records_to_read as usize];
        let buffer: &mut [u8] = bytemuck::cast_slice_mut(records_buffer);
        self.inner.read_exact(buffer)?;
        self.records_read += records_to_read;

        // return reference to it
        Ok(Some(records_buffer))
    }
}

#[cfg(test)]
mod test {
    use std::io::Cursor;

    use crate::io::xyz::XyzRecord;

    use super::*;

    #[test]
    fn test_writer_reader_many() {
        let cursor = Cursor::new(Vec::new());
        let mut writer = XyzInternalWriter::new(cursor);

        let record = XyzRecord {
            x: 1.0,
            y: 2.0,
            z: 3.0,
            classification: 4,
            number_of_returns: 5,
            return_number: 6,
            _padding: 0,
        };

        writer.write_records(&[record]).unwrap();
        writer.write_records(&[record]).unwrap();
        writer.write_records(&[record]).unwrap();

        // now read the records
        let data = writer.finish().unwrap().into_inner();
        let cursor = Cursor::new(data);
        let mut reader = super::XyzInternalReader::new(cursor).unwrap();
        let chunk = reader.next_chunk().unwrap().unwrap();

        assert_eq!(chunk.len(), 3);
        assert_eq!(chunk[0], record);
        assert_eq!(chunk[1], record);
        assert_eq!(chunk[2], record);
        assert_eq!(reader.next_chunk().unwrap(), None);
    }
}
