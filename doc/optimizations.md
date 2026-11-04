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
| Vegetaation vektorointi ja MVT-rakennus, yhdeksän lehteä | 77 s (Python), noin 3 GB | 5,3 s (Rust), 313 MB |
| Koko ketju yhdeksälle lehdelle (pullautin → MVT) | noin 365 s (arvio) | 150 s (Python) / **74 s (Rust)** |
| Iso lehti (`M4122F4`, 8,2 M pistettä), pullautin, `vector_output` | 7,26 s | 6,0 s (osio 7) |
| `vectortiles`, yhdeksän lehteä | 5,4 s (1 säie) | 4,3 s (2 säiettä), 3,8 s (kaikki 8 ydintä) (osio 8) |

Pullautinin tulosteet ovat **tavu-identtiset** alkuperäisen binäärin kanssa: molemmat PNG-kartat, kaikki väliaikaistiedostot ja Python-putkella tehdyt
yhdeksän lehden testialueen 72 MVT-tiiltä (PMTiles 2 382 581 tavua). Rust-versio MVT-tiilistä (osio 6) on tarkalleen sama viivoille ja pisteille ja
lähes sama (alle 0,2 % pinta-alasta) vegetaatiolle, mutta ei tavu-identtinen.

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

### 6. Python-vaiheet Rustiin: `pullauta vectortiles` (haara `vectortiles-rust`)

Komento `pullauta vectortiles <ajokansio> <ulostulo> [--no-tile-files] [--threads N] [--crs tm35fin|sweref99tm]` korvaa `proxy-server/mapant-tiles/`-skriptit (`stitch_veg.py`,
`vegetation_polygons.py`, `build_mvt.py`, `dxfparse.py`). Se lukee pullautinin ajojen `temp`-kansiot (ajo `vector_output=1`:llä): kerrosten
`.dxf.bin`-tiedostot, `vegetation.png` (indeksoitu PNG: paletti-indeksi kertoo luokan suoraan), `vegetation.pgw` ja `water.txt`. Tulos on
`map.pmtiles`, `metadata.json` ja halutessaan `tiles/z/x/y.pbf`.

Moduulit (`src/vectortiles/`): `project.rs` (TM35FIN → Web Mercator, Krüger-sarja; testi proj:n arvoja vastaan alle 1 mm), `features.rs` ja
`simplify.rs` (kerrosten luku, Douglas–Peucker viivoille, JTS:n renkaiden yksinkertaistus), `raster.rs` (mosaiikki, lohkoenemmistö,
morfologia, yhtenäisten alueiden tunnistus, ääriviivan jäljitys), `vegetation.rs`, `tiler.rs` (tiilijako, viivojen leikkaus Liang–Barskyllä,
polygonien leikkaus `geo`-kirjastolla), `mvt.rs` (käsin kirjoitettu protobuf) ja `pmtiles.rs` (PMTiles v3: Hilbert-tunnisteet, lehtihakemistot,
tiilidata virtaa väliaikaistiedostoon). Uudet riippuvuudet: `geo` (vain boolen-operaatiot ja ala) ja `flate2` (gzip). Moduuli on rajattu pois
wasm-käännöksestä.

| Yhdeksän lehden testialue | Python | Rust |
|---|---|---|
| liittäminen + vegetaatio + MVT | 4 + 53 + 24 = 81 s | 5,3 s (vegetaatio 3,6 s) |
| huippumuisti | noin 3 GB (mosaiikki) | 313 MB |

Tarkkuus (Rust verrattuna Python-tiiliin, 72 tiiltä):

| Kerros | Tulos |
|---|---|
| käyrät, muotoviivat, jyrkänteet, kumpareet | identtiset: piirteiden määrä, pistemäärä, pituus ja kumpujen koordinaatit täsmäävät kaikissa tiilissä |
| vegetaatio, alueet tasoittain | täsmälleen samat (19 815 / 2 081 / 10 052 / 3 250 / 3 316 / 402 / 111 vihreää aluetta tasoilla 1–7) |
| vegetaatio, tasoitetut polygonit | täsmäävät Pythonin kanssa alkio alkiolta (98,5 % alle 1e-6) |
| vegetaatio, lopulliset polygonit | 25 138 vs 25 130 polygonia; kokonaisala −0,18 %; pisteitä +0,8 %; pahin tiili 1,7 % |

Yksinkertaistus selittää loppuerot: GEOS:n `preserve_topology=True` ei ole pelkkä Douglas–Peucker. Se pakottaa renkaan ensimmäiset osat jaettaviksi
syvyyteen 3 asti, jotta renkaaseen jää vähintään neljä pistettä, ja pienet alueet kutistuvat siksi vähemmän (pienten polygonien ala 0,53 vs 0,46
alkuperäisestä). Rustissa tämä on toteutettu samalla säännöllä (`simplify_ring`), mutta ilman JTS:n leikkaustarkistuksia, ja renkaan
aloituspiste on eri, joten yksittäiset polygonit eivät ole identtisiä. Reikiä on 4 212 vs 4 658 (syytä en selvittänyt). Pythonin GeoJSON pyöristi
koordinaatit 0,1 m:iin, Rust ei.

Varmistus: Pythonin `pmtiles`-lukija lukee otsikon ja kaikki 72 tiiltä (täsmäävät kirjoitettuihin), OpenLayersin MVT-lukija jäsentää kaikki
piirteet, ja 24 uutta yksikkötestiä (projektio vs pyproj, tiilitunnisteet vs viiteratkaisu, MVT:n edestakainen koodaus, PMTiles-kirjoitus
lehtihakemistoineen, sulkeminen ja tasoitus vs scipy, sisäkkäisyyssääntö).

### 7. Käyrien muodostus: vain tason leikkaamat solut (`4b7a12f`)

Profilointi (`Timing`-osiot, 8,2 M pisteen lehti, 6,0 s) näytti, että `knoll detection part 1` (2,43 s) on käytännössä yksi kutsu:
`heightmap2contours` 0,3 m:n välillä (2,6 s; `xyz2heightmap` vie vain 0,18 s). `grid2contours` kävi jokaiselle korkeustasolle (208 kpl)
läpi koko 2,25 M solun ruudukon, vaikka suurin osa soluista ei leikkaa kyseistä tasoa.

Solut jaetaan nyt tasoihin ennen silmukkaa: solu kuuluu niille tasoille, jotka ovat sen kulmien pienimmän ja suurimman arvon välissä (samat
vertailut kuin vanhassa ohituksessa), ja solut käydään samassa i, j -järjestyksessä. Tasot lasketaan samalla `level += v` -summauksella kuin
ennen. Tulos on **tavu-identtinen** (kaikki 11 vertailtua tiedostoa: `.dxf.bin` ja `.hmap`).

| | Ennen | Nyt |
|---|---|---|
| `knoll detection part 1` | 2,43 s | 1,23 s |
| `contour generation part 2` (2,5 m:n käyrät) | 0,30 s | 0,17 s |
| koko lehti | 7,26 s | 6,0 s |

Jäljelle jäävä noin 1 s on segmenttien haku (0,58 s, 8,1 M päätepistettä) ja polylinejen ketjutus HashMapin avulla (0,31 s). Näitä ei muutettu
(päätös: ei toteuteta). Mahdollisia jatkoja olisivat tasojen rinnakkaisajo ja HashMapin korvaaminen ruudukkoindeksillä.

#### Aikajakauma tämän jälkeen (iso lehti, kolme ajoa, 6,0 s)

| Vaihe | Aika |
|---|---|
| vegetation generation | 2,0 s |
| knoll detection part 1 (`contours03`) | 1,23 s |
| LAZ:n luku (`preparing input file`) | 0,94 s |
| cliff generation | 0,73 s |
| contour generation 1–4 | 0,55 s |
| knoll detection part 2 | 0,22 s |
| water cells | 0,16 s |
| DXF/bin-tulosteet, muotoviivat | 0,16 s |

Rinnakkaisuutta ei ole muualla kuin kahdessa asetuksessa: `processes` (lehtiä rinnakkain, vaatii `batch=1`) ja `parallel_laz_decompression`.
Yhden lehden sisällä vaiheet ovat yksisäikeisiä (`vector_output` ohittaa PNG-renderöinnin, jossa `image`-kirjaston rayon oli ainoa rinnakkaisuus), joten
yksi lehti käyttää noin 1,5 ydintä 8:sta.

### 8. Vegetaatiopolygonit usealla säikeellä: `--threads` (`6a03af6`, `4437d7c`)

`vectortiles`-vaiheen vegetaatio (3,6 s yhdeksälle lehdelle) jakautui näin: ruudukko 0,54 s, alueiden nimeäminen 0,13 s, jäljitys 0,44 s, pehmennys
0,58 s, union 0,52 s, leikkaus vanhempaan 0,78 s, yksinkertaistus ja projektio 0,33 s.

`vegetation::polygons` ajaa nyt kolme vaihetta `rayon`-poolissa: (1) jäljitys, pehmennys ja union alueittain, (2) leikkaus vanhempaan
sisäkkäisyyssyvyyden mukaan (vanhempi on aina ennen lasta, joten saman syvyyden alueet vain lukevat edellisen syvyyden valmiita alueita), (3)
yksinkertaistus ja projektio. Tulokset kerätään alkuperäisessä järjestyksessä, joten **`map.pmtiles` on tavu-identtinen kaikilla säiemäärillä** (1, 2, 4, 0).

Parametri: `--threads N` (`BuildOptions::threads`); 0 = kaikki ytimet, 1 = yksi säie, **oletus 2**.

| Säikeitä | Seinäaika | Muisti |
|---|---|---|
| 1 | 5,50 s | 369 MB |
| 2 | 4,26 s | 386 MB |
| 4 | 3,89 s | 411 MB |
| 0 (8 ydintä) | 3,82 s | 472 MB |

Nopeutus jää pieneksi (1,44×), koska ruudukon rakennus, nimeäminen ja luku ovat yksisäikeisiä ja yksi suuri alue hallitsee rinnakkaista osaa.
Oletukseksi valittiin 2, jotta `vectortiles` ei vie kaikkia ytimiä muulta työltä.

### 9. SWEREF 99 TM (`cc843ed`)

`project.rs` ottaa keskimeridiaanin parametrina (TM35FIN 27 °, SWEREF 99 TM 15 °); GRS80, mittakaava 0,9996 ja false easting ovat samat.
Komennolle `--crs sweref99tm` (oletus `tm35fin`). Testit: kolme pistettä pyprojin `EPSG:3006 → 3857` -tulosta vastaan alle 1 mm, ja nimien jäsennys.
Tiilien `metadata.json`-rajat osuivat odotettuihin koordinaatteihin. Testejä on nyt 96.

### 10. Yleiskuvataso z13 (`6b61304`)

Pelkkä z14 oli liian raskas pienillä zoomeilla: sama maa-ala vaatii nelinkertaisen määrän z14-tiiliä, ja zoomilla 13 piirrettäisiin kymmeniä
tuhansia viivoja kerralla. `pullauta vectortiles` tekee nyt z14:n lisäksi **z13-tiilet** (`--overview-zoom N`, oletus 13, `none` = vain z14),
joissa on vähemmän ja yksinkertaisempia piirteitä (`Features::overview`, Web Mercator -metreinä): vain `normal`- ja `index`-käyrät (tarkkuus 3 m,
vähintään 60 m pitkät), jyrkänteet `cliff2`/`cliff3` (vähintään 25 m), vegetaatiopolygonit ja reiät vähintään 1600 m2 yksinkertaistettuna 6 m:llä; ei
muotoviivoja eikä kumpuja. PMTiles-arkistossa on molemmat tasot (matalamman tason tiilet ensin, koska tunnisteet järjestävät niin).

| Testialue | Ilman z13 | z13 + z14 |
|---|---|---|
| Tampere, 9 lehteä (`map.pmtiles`) | 2,38 MB, 72 tiiltä | 2,83 MB, 94 tiiltä (z13: 22 tiiltä, 0,57 MB raakana, 5285 käyrää, 3310 polygonia) |
| Ruotsi, 14 lohkoa | 13,6 MB | 16,7 MB |
| Rakennusaika | 5,2 s | 5,0 s (Tampere); Ruotsin lohkot 237 s (2 rinnakkain) |

Yksi z13-tiili kattaa nelinkertaisen alan, mutta on kooltaan noin saman kuin z14-tiili (Tampere 31,6 kt vs 36,2 kt, Skellefteå 36,9 kt vs 26,3 kt), eli
alaa kohden noin 4 kertaa kevyempi. Tile2:n palvelin tukee useita zoomeja arkistoissaan (tarkistaa otsikon minZ..maxZ), joten palvelimeen ei tarvittu muutosta.

### 11. Hidas lohko profiloitu: leikkaus vanhempaan (`ac5fb87`)

Ruotsin Skellefteån keskustalohko (10 arkkia, 62 km², 26 200 polygonia) kesti 125 s, Tampereen 81 km² 5 s. Profilointi (yksi säie): lukeminen 7,8 s,
ruudukko 0,5 s, ääriviivat/pehmennys/union 2,6 s, **leikkaus vanhempaan 122,8 s**, yksinkertaistus ja projektio 0,6 s. Sisäkkäisen alueen leikkaus
`geo`-kirjaston boolen-operaatiolla vanhempaansa on hidas, kun vanhempi on iso (metsä, jossa tuhansia aukkoja: kymmeniä tuhansia pisteitä) ja lapsia on tuhansia.

Korjaus: lasta ei leikata, jos vanhemmalla on yli 4000 pistettä (lapsi on rasterissa vanhemman osajoukko, joten se voi työntyä pehmennetyn vanhemman
reunan yli korkeintaan sen metrin, jonka pehmennys siirtää ääriviivaa), eikä silloin, kun lapsi on rasterissa yli 3σ:n päässä vanhemman reunasta.
Lohko 146 s → **28 s** yhdellä säikeellä, vegetaation pinta-ala muuttui 0,0002 %. Tampereen tulos on tavu-identtinen. Kaikki 14 Ruotsin lohkoa:
237 s → **84 s** (2 rinnakkain, 2 säiettä; CPU 470 s → 63 s).

### 12. Jyrkänteiden pätkien yhdistäminen (`ac5fb87`)

Pullautin piirtää jyrkänteet lyhyinä (kahden pisteen) pätkinä. Saman lajin (`cliff2`/`cliff3`/`cliff4`) pätkät, joiden päätepisteet ovat alle 2 m:n päässä
toisistaan, yhdistetään ketjuiksi (`join_lines`: ahne, lähin päätepiste ensin hännästä ja sitten päästä) ja ketjut yksinkertaistetaan 0,5 m:llä.
Tampere: 39 589 piirrettä / 79 178 pistettä → **6 510 / 52 054**; tiilet 4,33 → 3,81 MB raakana. Z13-yleiskuvassa jyrkänteet ovat nyt pidempiä ketjuja, joten
25 m:n alaraja ei karsi lähes kaikkea (1 223 jyrkännettä).

## Aineisto: pistetiheys ja muut maat (mittaukset ja havainnot)

Tulokset ovat havaintoja yksittäisistä alueista; **johtopäätökset ovat arvioita**, ja kynnysten säätöä ei ole kokeiltu.

**MML:n aineisto (Funet):** 0,70–0,91 pistettä/m² (yhdeksän lehteä, kaikki paluut; LAZ-otsikoista).

**5p-aineisto (MML `Avoin_laseraineisto5p`):** vain viisi rajattua tuotantoaluetta (Forssa, Nurmijärvi, Nuuksio, Pieksämäki, Suomutunturi),
ei koko maata. Nuuksion lehti L4132F3 (9 × 1 km, 9,6–14,1 pistettä/m², korkeus 213 m asti) täydellä tiheydellä ja ohennettuna (`thinfactor 0.08`, noin 0,8/m²):

| | 5p täysi | Ohennettu |
|---|---|---|
| korkeuskäyrät | 1518 (1 065 137 m) | 1553 (1 058 979 m, −0,6 %) |
| jyrkänteet | **350 385** | 10 237 |
| vegetaation pinta-ala | 124 M | 91 M (−27 %) |
| tiilet yhteensä | 6,69 MB | 0,57 MB |
| pullautin / lehti (1 km²) | 2,8–4,4 s, 194–248 MB | 1,4–2,1 s, 83–90 MB |

Korkeuskäyriin 0,8 pistettä/m² riittää. Aluskasvillisuus (vihreä) kärsii harvasta aineistosta, ja tiheällä aineistolla jyrkänteitä tulee moninkertaisesti
(oletuskynnykset on viritetty harvalle aineistolle; arvio, ei testattu).

**Lantmäteriet Laserdata Skog (Ruotsi):** STAC-rajapinta `https://api.lantmateriet.se/stac-hojd/v1`, kokoelma `dsm-skoglig-copc`, COPC-LAZ (LAS 1.4,
pistemuoto 6), 2,5 km:n tiilet, EPSG:5845 (SWEREF 99 TM), CC BY 4.0. Tiedostojen lataus vaatii Basic-tunnistuksen. Pullautin lukee tiedostot sellaisenaan.

| Tiili | Alue | Tiheys | Pullautin | Jyrkänteet |
|---|---|---|---|---|
| `y20d027-67725_5050_25` | Dalarna | 1,1/m² | 7,4 s, 482 MB | 23 |
| `y21d009-66625_4125_25` | Värmland | 2,1/m² | 5,8 s, 350 MB | 866 |
| `y22a035-63500_4900_25` | Småland | 0,86/m² | 10,3 s, 566 MB | 103 842 |
| `y26h019-73725_8900_25` | Norrbotten | 1,3/m² | 6,4 s, 455 MB | 4 468 |

**Vertailu Gokartorin tiiliin** (`https://t.gokartor.se/Master/{z}/{y}/{x}.png`, EPSG:3006, resoluutiot 16384 … 1 … 0,5 m/px zoomilla 0…15, origo
(265000, 7680000), y ennen x:ää; vertailtu zoom 15:llä, 640 m:n ruudut; Småland ja Dalarna):
- korkeuskäyrien muoto täsmää erinomaisesti; Gokartor piirtää paksummat välikäyrät
- jyrkänteiden sijainnit täsmäävät (Småland); Gokartorin symbolit ovat paksumpia
- meidän vihreämme on selvästi runsaampaa ja tummempaa kuin Gokartorin (aluskasvillisuuden kynnykset on viritetty MML:n aineistolle; arvio)
- meiltä puuttuvat tiet, purot, vesi, suot ja rakennukset, koska ne eivät tule pistepilvestä

## Koko ketju (yhdeksän lehden testialue)

| Vaihe | Aika |
|---|---|
| pullautin, yhdeksän lehteä peräkkäin | 69 s |
| `stitch_veg.py` (mosaiikki ja vesiruudut) | 4 s |
| `vegetation_polygons.py` (mosaiikki) | 53 s |
| `build_mvt.py` | 24 s |
| **Yhteensä** | **150 s (16,7 s per lehti)** |

Python-osuus oli yli puolet ajasta (cProfile ei näyttänyt yksittäistä pullonkaulaa). Se on siirretty Rustiin (osio 6): `pullauta vectortiles`
vie samalle alueelle **5,3 s**, jolloin koko ketju on pullautin 69 s + 5,3 s = **74 s (8,2 s per lehti)**.

## Aineiston kokoon liittyvät valinnat (tiilet)

Nämä eivät muuta pullautinia vaan MVT-tiilien kokoa (`proxy-server/mapant-tiles/`, `proxy-server/doc/mapant-vector-tiles.md`):

- **Vegetaatio on vektoria 3 m:n ruudukosta.** Pienet polygonit ja reiät (alle 36 m²) pudotetaan, vesi rajataan pois, keltainen suljetaan 15 m:n
  raoilta ja renkaat tasoitetaan (σ = 6 m) ja yksinkertaistetaan (2 m). Testilehdellä `M4211E3` MVT-tyyppinen arvio 72 kt → 59 kt (tasoitus) →
  45 kt (keltaisen sulkeminen); metsäisellä `V4133E4`:llä 108 kt → 59 kt.
- **Käyriä yksinkertaistetaan 0,3 m:llä** (Douglas–Peucker) ennen projisointia; pullautin on jo tasoittanut ja harventanut ne.
- **Vain z14 tallennetaan.** OpenLayers hakee samat tiilet myös zoomeilla alle 14, joten alempia tasoja ei tarvita. Testialue on yhteensä 2,38 MB (72 tiiltä,
  mediaani 37 kt).

## Tarkistus ja toistaminen

- `cargo fmt --check`, `cargo clippy --release` (ei varoituksia) ja `cargo test --release` (96 testiä) menevät läpi.
- Uudet yksikkötestit: `nearest_view_matches_image_crate` ja `nearest_view_uses_only_the_cropped_part` (`src/render.rs`).
- Vertailu: aja alkuperäinen ja uusi binääri samalla LAZ-tiedostolla eri hakemistoihin ja vertaa (`cmp`) `pullautus.png`, `pullautus_depr.png` ja kaikki
  `temp/`-tiedostot. Vektoritilassa vertaa `out2.dxf`, `formlines.dxf`, `c2g.dxf`, `c3g.dxf`, `dotknolls.dxf` ja `vegetation.png`.
- Ajojen aikojen lukeminen: `RUST_LOG=debug pullauta <tiedosto>.laz` ja rivit `Leaving section`.

## Mitä ei tehty

- GPU-laskentaa ei kokeiltu: raskaat vaiheet ovat haarautuvaa geometriaa, ja mittaus osoitti pullonkaulat muualla (silmukat, kopiot).
- Rust-version polygonit eivät ole tavu-identtisiä Pythonin kanssa (yksinkertaistus ilman JTS:n leikkaustarkistuksia), ja suurille alueille (koko maa)
  rakennus pitäisi tehdä lohkoittain: nyt kaikki piirteet ovat muistissa kerralla.
- Konfiguraatiota `thinfactor` (pisteiden harvennus) ei käytetty tuotannossa, koska se muuttaisi tulosta; sitä käytettiin vain tiheyden vaikutuksen mittaamiseen.
- Käyrien segmenttihaun ja ketjutuksen nopeutusta (tasojen rinnakkaisajo, ruudukkoindeksi HashMapin tilalle) ei toteutettu.
- Aluskasvillisuuden ja jyrkänteiden kynnyksiä ei viritetty 5p- tai Ruotsin aineistolle.
- Käyräpiirron leimausta ei muutettu, koska mittaus ei osoittanut sitä pullonkaulaksi.

## Commitit (haarat `render-optimization` ja `vectortiles-rust`)

| Commit | Sisältö |
|---|---|
| `60d5f5a` | `render_both`, `render_nodepressions`, `resize_nearest` |
| `e5311fe` | `NearestView`, rivi kerrallaan luettu undergrowth, pohjan uudelleenrakennus |
| `71617b9` | kumpareiden tunnistus: `PointGrid` ja `Vec`-korkeudet |
| `1ad340b` | `vector_output`, `watercells`, DXF vain lopullisista kerroksista |
| `c7161d8`, `672236e`, `621c667` | haara `vectortiles-rust`: projektio, MVT, PMTiles, kerrokset, vegetaatio ja `vectortiles`-komento |
| `0b0e242` | tämä dokumentti: Python-vaiheet Rustiin |
| `4b7a12f` | käyrät: vain tason leikkaamat solut |
| `6a03af6`, `4437d7c` | vegetaatio usealla säikeellä, `--threads` (oletus 2) |
| `cc843ed` | SWEREF 99 TM, `--crs` |
| `6b61304` | z13-yleiskuvataso, `--overview-zoom` |
| `ac5fb87` | jyrkänteiden pätkien yhdistäminen, leikkaus vain pienelle vanhemmalle |
