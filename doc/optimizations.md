# Optimoinnit: renderöinti, kumpareet ja vektorituotanto

Tämä dokumentti kuvaa haaran `render-optimization` muutokset ja niiden mittaukset. Tavoite oli, että yhden 3 × 3 km:n lehden käsittely vie
vähemmän aikaa ja muistia, ja että vektoriaineisto (MVT-tiilet) syntyy ilman tarpeettomia välivaiheita. Kaikki luvut on mitattu tässä
työssä samalla työasemalla (8 ydintä) MML:n laserkeilausaineistolla (Funetin peili), ellei toisin mainita. **Arviot on merkitty arvioiksi.**

## Yhteenveto

| Mittari | Ennen | Nyt |
|---|---|---|
| Iso lehti (`M4211E3`, 8,1 M pistettä), pullautin | 24,2 s | 8,0 s (`vector_output`) / 9,9 s (normaali ajo) |
| Pienempi lehti (`K3414B1`, 4,3 M pistettä), pullautin | 22,8 s, 1,40 GB | 11,1 s, 0,45 GB |
| Huippumuisti, iso lehti | 466 MB | 315 MB (`vector_output`) |
| Vesiruudut vektorituotantoa varten | 9,2 s | 0,16 s |
| Koko ketju yhdeksälle lehdelle (pullautin → MVT) | noin 365 s (arvio) | 150 s |

Tulosteet ovat **tavu-identtiset** alkuperäisen binäärin kanssa: molemmat PNG-kartat, kaikki väliaikaistiedostot ja yhdeksän lehden testialueen
72 MVT-tiiltä (PMTiles 2 382 581 tavua).

## Mittauspohja

Pullautinissa on valmiina `Timing`-osiot (`RUST_LOG=debug`). Alkuperäisellä binäärillä pienen lehden (4,3 M pistettä) 22,8 sekunnista meni
renderöintiin 13,8 s (7,4 s + 6,4 s, painanteellinen ja painanteeton kartta). Ison lehden (8,1 M pistettä) 24,2 sekunnista kumpareiden
tunnistus (`knoll detection part 2`) vei 14,6 s.

Ennen jokaista korjausta mitattiin, mihin aika oikeasti kuluu. Kaksi alkuperäistä oletusta osoittautui vääräksi:

- **PNG-dekoodaus ei ollut hidas** (50–70 ms per tiedosto). Hidas oli `image`-kirjaston `resize` (3,4 s per 3003 → 7093 px -skaalaus).
- **Käyrien piirto ei ollut hidas** (alle 0,1 s). Oletin, että jokaisen pisteen leimaaminen 25–36 kertaa olisi pullonkaula, mutta mittaus ei
  tukenut sitä.

## Muutokset

### 1. Renderöinti jaettiin kahden kartan kesken (`60d5f5a`)

Pullautin tuottaa kaksi PNG-karttaa (painanteilla ja ilman). Kumpikin kutsu dekoodasi ja skaalasi samat kerrokset ja laski korkeuskentän
jyrkkyyden alusta. Nyt:

- `render_both` rakentaa pohjan (kasvillisuus, aluskasvillisuus, low, pohjoisviivat) ja lukee korkeuskentän ja jyrkkyyden kerran
  (`load_curve_inputs`). Painanteellinen kartta tehdään ensin, koska se kirjoittaa `formlines.dxf.bin`-tiedoston.
- Dekoodatut PNG:t vapautetaan heti käytön jälkeen (`into_rgba8()`, ei kopioita).
- Uusi ini-asetus `render_nodepressions` (oletus `1`). Arvolla `0` tehdään vain painanteellinen kartta.
- `Timing`-osiot `render_both`-funktiossa.

Tulos (`K3414B1`): 22,8 s → 19,7 s, 1,40 GB → 1,07 GB. Asetuksella `render_nodepressions=0`: 16,4 s ja 875 MB.

### 2. Nearest-skaalaus ilman f32-välivaihetta (`60d5f5a`)

`image::imageops::resize` käyttää suodattimesta riippumatta f32-välikuvaa (`Rgba32F`). Skaalauksessa 3003² → 7093² se varasi noin 340 MB
lisää ja vei 3,4 s. Nearest-suodattimella jokainen ulostulopikseli on yksi lähdepikseli painolla 1, joten arvo säilyy kokonaislukuna.
Korvaava koodi laskee lähdepikselin samalla f32-kaavalla (`floor((x + 0,5) * suhde)`) ja kopioi suoraan u8-puskuriin. Yksikkötestit vertaavat
tulosta kirjaston tulokseen eri kokoille ja rajatulle lähdealueelle.

Tulos: 19,7 s → 11,0 s, 1,07 GB → 0,70 GB. Pohjakerrokset 3,8 s → 0,96 s ja päällekkäiset kerrokset 3,0 s → 0,3 s per versio.

### 3. Kerrokset suoraan kankaalle (`e5311fe`)

- `NearestView` (toteuttaa `GenericImageView`) lukee lähdepikselit vasta piirtohetkellä, joten skaalattuja kopioita ei tehdä. `image::imageops::overlay`
  käyttää sitä sellaisenaan, joten sekoituslogiikka on täsmälleen sama.
- `undergrowth.png` sekoitetaan rivi kerrallaan `png`-kirjastolla, kun sen koko vastaa kangasta (muuten käytetään vanhaa polkua). Koko kuvaa ei
  koskaan pidetä muistissa dekoodattuna (säästö noin 200 MB).
- Painanteettoman kartan pohja rakennetaan uudelleen (noin 0,4 s) kopion säilyttämisen (noin 200 MB) sijaan.

Tulos: 703 MB → 452 MB, ja samalla kokonaisaika pysyi 11 sekunnissa. Muut tilet (vanha → uusi): `L3313A4` 17,3 → 5,3 s ja 1,37 → 0,45 GB,
`R4233C4` 15,9 → 4,8 s ja 1,34 → 0,44 GB, `V4133E4` 28,7 → 18,1 s ja 1,42 → 0,51 GB.

### 4. Kumpareiden tunnistus: kaikkien parien silmukat pois (`71617b9`)

Ison lehden mittauksessa kumpareiden tunnistus vei 14,6 s. Lisätyt `Timing`-osiot (`knolldetector`) paikansivat kaksi silmukkaa
(`bounding boxes and tops` 6,5 s ja `candidates` 8,1 s), joissa jokaista viivaa verrattiin kaikkiin pisteisiin ja korkeus haettiin kahdesti
hajautustaulusta. Muut osat vievät yhteensä alle 0,3 s.

- `elevation` on nyt `Vec<f64>` viivan indeksillä (hajautustaulun tilalla).
- `PointGrid` (100 m:n tasaruudukko) hakee rajauslaatikon sisällä olevat pisteet. Hakutulos on järjestetty alkuperäiseen listajärjestykseen, joten
  ensimmäinen osuma (`topid`) ja "mikä tahansa osuma" (`skip`) ovat samat kuin ennen.

Tulos: molemmat silmukat noin 20 ms. Iso lehti 24,2 s → 9,9 s. Kaikki väliaikaistiedostot ja PNG:t tavu-identtiset.

### 5. Tila `vector_output` (`1ad340b`)

Vektoriaineisto ei tarvitse PNG-karttoja eikä kaikkia tekstimuotoisia DXF-tiedostoja. Ini-asetus `vector_output = 1` (käytä yhdessä
`output_dxf=0`:n kanssa):

- **ohittaa PNG-renderöinnin.** Muotoviivat (`formlines.dxf.bin`) tuotetaan silti `draw_curves`-funktiolla ilman kuvaa.
- **laskee vesiruudut pullautinissa** (`vegetation::watercells` → `temp/water.txt`): 3 m:n ruudut, joissa on vesiluokan pisteitä, rivinä
  `x y vesi yhteensä`. Aiempi tapa muunsi pisteet tekstiksi (`internal2xyz`) ja laski `awk`:lla: 9,2 s → 0,16 s, ja tuloksena samat 2994 ruutua.
- **kirjoittaa tekstimuotoisen DXF:n vain viidestä kerroksesta** (`out2`, `formlines`, `c2g`, `c3g`, `dotknolls`). Muiden vaiheiden DXF:t jäävät
  pois: pelkkä `contours03.dxf` oli 144,6 MB tekstiä (muut: `out.dxf` 15,9 MB, `out2.dxf` 19,7 MB, `formlines.dxf` 6,9 MB).

Tulos (iso lehti): 9,9 s → 8,0 s, 466 MB → 315 MB. Viisi DXF-tiedostoa ja `vegetation.png` ovat tavu-identtiset normaaliajon kanssa.

## Koko ketju (yhdeksän lehden testialue)

| Vaihe | Aika |
|---|---|
| pullautin, yhdeksän lehteä peräkkäin | 69 s |
| `stitch_veg.py` (mosaiikki ja vesiruudut) | 4 s |
| `vegetation_polygons.py` (mosaiikki) | 53 s |
| `build_mvt.py` | 24 s |
| **Yhteensä** | **150 s (16,7 s per lehti)** |

Jäljellä oleva Python-osuus on yli puolet ajasta. cProfile ei näytä yksittäistä pullonkaulaa: kirjastojen lataus 1,2 s, tasoitus 1,9 s,
suorakulmioiden yhdistäminen noin 1,5 s. Merkittävä nopeutus vaatisi Rust-portin (`.dxf.bin`-tiedostojen ja luokkaruudukon suora luku, MVT:n
kirjoitus). Sen arvioin tuovan lehden aikaan noin 10 s; sitä ei ole tehty.

## Aineiston kokoon liittyvät valinnat (tiilet)

Nämä eivät muuta pullautinia vaan MVT-tiilien kokoa (`proxy-server/mapant-tiles/`, `proxy-server/doc/mapant-vector-tiles.md`):

- **Vegetaatio on vektoria 3 m:n ruudukosta.** Pienet polygonit ja reiät (alle 36 m²) pudotetaan, vesi rajataan pois, keltainen suljetaan 15 m:n
  raoilta ja renkaat tasoitetaan (σ = 6 m) ja yksinkertaistetaan (2 m). Testilehdellä `M4211E3` MVT-tyyppinen arvio 72 kt → 59 kt (tasoitus) →
  45 kt (keltaisen sulkeminen); metsäisellä `V4133E4`:llä 108 kt → 59 kt.
- **Käyriä yksinkertaistetaan 0,3 m:llä** (Douglas–Peucker) ennen projisointia; pullautin on jo tasoittanut ja harventanut ne.
- **Vain z14 tallennetaan.** OpenLayers hakee samat tiilet myös zoomeilla alle 14, joten alempia tasoja ei tarvita. Testialue on yhteensä 2,38 MB (72 tiiltä,
  mediaani 37 kt).

## Tarkistus ja toistaminen

- `cargo fmt --check`, `cargo clippy --release` (ei varoituksia) ja `cargo test --release` (70 testiä) menevät läpi.
- Uudet yksikkötestit: `nearest_view_matches_image_crate` ja `nearest_view_uses_only_the_cropped_part` (`src/render.rs`).
- Vertailu: aja alkuperäinen ja uusi binääri samalla LAZ-tiedostolla eri hakemistoihin ja vertaa (`cmp`) `pullautus.png`, `pullautus_depr.png` ja kaikki
  `temp/`-tiedostot. Vektoritilassa vertaa `out2.dxf`, `formlines.dxf`, `c2g.dxf`, `c3g.dxf`, `dotknolls.dxf` ja `vegetation.png`.
- Ajojen aikojen lukeminen: `RUST_LOG=debug pullauta <tiedosto>.laz` ja rivit `Leaving section`.

## Mitä ei tehty

- GPU-laskentaa ei kokeiltu: raskaat vaiheet ovat haarautuvaa geometriaa, ja mittaus osoitti pullonkaulat muualla (silmukat, kopiot).
- Vegetaation vektorointia ja MVT-rakennusta ei ole siirretty Rustiin (ks. yllä).
- Konfiguraatiota `thinfactor` (pisteiden harvennus) ei käytetty, koska se muuttaisi tulosta.
- Käyräpiirron leimausta ei muutettu, koska mittaus ei osoittanut sitä pullonkaulaksi.

## Commitit (haara `render-optimization`)

| Commit | Sisältö |
|---|---|
| `60d5f5a` | `render_both`, `render_nodepressions`, `resize_nearest` |
| `e5311fe` | `NearestView`, rivi kerrallaan luettu undergrowth, pohjan uudelleenrakennus |
| `71617b9` | kumpareiden tunnistus: `PointGrid` ja `Vec`-korkeudet |
| `1ad340b` | `vector_output`, `watercells`, DXF vain lopullisista kerroksista |
