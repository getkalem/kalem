# Kalem — İş Listesi

Bu liste `design_document.md`'den türetilmiştir. Her madde ilgili bölüme `§` ile atıf yapar. Tasarım değişince önce belge, sonra bu liste güncellenir.

İşaretler: `[ ]` yapılacak · `[x]` bitti · `[-]` iptal · `[~]` devam ediyor
Kimlikler: `T<faz>.<grup>.<sıra>`; kararlar `D<n>` (§21).

Fazlar sıralıdır. Bir fazın **çıkış ölçütleri** karşılanmadan sonrakine geçilmez.

---

## Şu an: ilk on iş

1. [ ] T0.1.1 Ad kontrolü: crates.io'da `kalem`, GitHub'da `kalem` uygunluğu; alternatif `kalem-editor` (D7)
2. [ ] T0.1.2 Lisans kararı (D1)
3. [ ] T0.1.3 Depo ve Cargo workspace iskeleti
4. [ ] T0.2.1 orgize değerlendirmesine başla (D2)
5. [ ] T0.4.1 Emacs `dump.el` betiği
6. [ ] T0.4.5 Korpus toplama: Org Manual kaynağı
7. [ ] T0.6.1 gpui spike deposu (D3)
8. [ ] T0.7.1 mitex + typst prototipi (D4)
9. [ ] T0.1.6 Tasarım belgesini `rfcs/0001-kalem.md` olarak taşı
10. [ ] TS.1 Tasarım belgesinin İngilizce çevirisi (yayın öncesi)

---

## Faz 0: Keşif ve temel (§20, 2 ila 3 ay)

### 0.1 Proje kurulumu (§18)

- [ ] T0.1.1 Ad kontrolü: crates.io, GitHub, alan adı; sonuç D7'ye işlenir
- [ ] T0.1.2 Lisans kararı D1; `LICENSE-MIT`, `LICENSE-APACHE` (ya da GPL) dosyaları
- [ ] T0.1.3 Cargo workspace: `crates/org-syntax`, `crates/kalem-cli` başlangıç; `rust-toolchain.toml`; `rustfmt.toml`; `clippy` ayarları
- [ ] T0.1.4 CI: GitHub Actions, Linux + macOS + Windows matrisi; fmt, clippy, test; cache
- [ ] T0.1.5 `README.md` (kısa vizyon, "Org için Typora"), `CONTRIBUTING.md`, davranış kuralları, PR ve issue şablonları, `CHANGELOG.md`
- [ ] T0.1.6 `rfcs/0001-kalem.md`: tasarım belgesi RFC olarak; RFC süreci kısa açıklaması
- [ ] T0.1.7 `tests/corpus/LICENSES.md`: korpus lisans kayıt düzeni
- [ ] T0.1.8 Org geliştirici listesine ilk duyuru taslağı (yayın Faz 1 sonunda)

### 0.2 Parser temeli kararı (§5.6, D2)

- [ ] T0.2.1 orgize'ı workspace'e deneme bağımlılığı olarak ekle; sürüm ve rowan tabanlılığı doğrula
- [ ] T0.2.2 Korpus üzerinde round-trip testi (`parse(x).to_string() == x`)
- [ ] T0.2.3 §3.2 tablosuna karşı kapsam denetimi; eksik öğe listesi
- [ ] T0.2.4 Artımlı yeniden ayrıştırma var mı, eklenebilir mi (mimari inceleme)
- [ ] T0.2.5 Bakım durumu, açık PR süresi, lisans
- [ ] T0.2.6 Karar notu: bağımlılık / fork / sıfırdan; D2 kapatılır

### 0.3 org-syntax (§5)

- [ ] T0.3.1 `SyntaxKind` enum'u: token ve düğüm türleri, §3.2'deki her öğe
- [ ] T0.3.2 Lexer: satır tabanlı token'lar, boşluk ve satır sonu korunur (LF ve CRLF)
- [ ] T0.3.3 `ParseContext` ve ön geçiş: `#+TODO`, `#+TAGS`, `#+STARTUP`, `#+PROPERTY`, `#+LINK`, `#+MACRO`, `#+CONSTANTS`, `#+SETUPFILE` (enjekte edilen yükleyici)
- [ ] T0.3.4 Eleman parser'ı: headline ve section
- [ ] T0.3.5 Eleman parser'ı: planning, property drawer, drawer
- [ ] T0.3.6 Eleman parser'ı: plain list, item, checkbox, tanım listesi, girinti kuralları
- [ ] T0.3.7 Eleman parser'ı: table (org) satır ve hücreler, hline, `#+TBLFM`; table.el tanıma
- [ ] T0.3.8 Eleman parser'ı: bloklar (src, example, export, verse, comment, center, quote, special), dynamic block, kapanmamış blok toleransı
- [ ] T0.3.9 Eleman parser'ı: keyword, affiliated keyword, babel call, clock, diary sexp, latex environment, fixed-width, horizontal rule, comment, footnote definition, inlinetask
- [ ] T0.3.10 Nesne parser'ı: vurgular (Org regex kuralları birebir), code, verbatim
- [ ] T0.3.11 Nesne parser'ı: bağlantı türleri (file, http, id, custom-id, fuzzy, radio, coderef, kısaltma, attachment), plain ve angle link
- [ ] T0.3.12 Nesne parser'ı: timestamp (aktif, pasif, aralık, tekrar, uyarı, diary)
- [ ] T0.3.13 Nesne parser'ı: footnote reference, inline src, inline call, latex fragment, entity, sub/superscript, line break, macro, export snippet, target, radio target, statistics cookie, citation
- [ ] T0.3.14 Tipli AST katmanı (`ast::Headline` vb.), §5.5 API
- [ ] T0.3.15 Artımlı yeniden ayrıştırma: bölüm granülerliği, sınır değişimleri, ön geçiş değişiminde tam parse
- [ ] T0.3.16 Hata toleransı: `SyntaxError` listesi, panic yok garantisi
- [ ] T0.3.17 Snapshot testleri (`insta`): her öğe için en az üç örnek
- [ ] T0.3.18 Round-trip fuzz (`cargo-fuzz`) ve `proptest` rastgele belge üretici
- [ ] T0.3.19 Benchmark (`criterion`): 1 MB ve 10 MB tam parse, tipik artımlı düzenleme; hedef §15
- [ ] T0.3.20 `org-syntax` crate belgeleri ve `docs.rs` hazırlığı

### 0.4 Emacs diferansiyel test altyapısı (§5.7, §16)

- [ ] T0.4.1 `tests/emacs/dump.el`: `org-element-parse-buffer` → JSON (tür, begin, end, temel özellikler)
- [ ] T0.4.2 Rust tarafı JSON dökümü (`kalem-cli dump --format emacs-json`)
- [ ] T0.4.3 Normalleştirme ve karşılaştırma aracı; fark raporu
- [ ] T0.4.4 CI: Emacs kurulu container, korpus üzerinde diferansiyel koşu; eşik yüzde 99
- [ ] T0.4.5 Korpus: Org Manual `.org` kaynağı, Worg sayfaları, izinli topluluk dosyaları, sentetik uç durumlar
- [ ] T0.4.6 Bilinen farklar belgesi (`docs/known-differences.org`)

### 0.5 kalem-cli (§4.2)

- [ ] T0.5.1 `kalem parse <dosya>`: ağaç dökümü
- [ ] T0.5.2 `kalem check <dosya>`: sözdizimi hataları, round-trip doğrulama
- [ ] T0.5.3 `kalem diff-emacs <dosya>`: diferansiyel karşılaştırma
- [ ] T0.5.4 `kalem fmt` iskeleti (yalnızca tablo hizalama, Faz 2'de genişler)

### 0.6 gpui spike (§7.1, D3)

- [ ] T0.6.1 Ayrı spike deposu; gpui sürümü sabitlenir
- [ ] T0.6.2 Rope destekli, satır içi stilli düzenlenebilir paragraflar
- [ ] T0.6.3 IME kompozisyonu: macOS, Windows, Linux
- [ ] T0.6.4 Yüz bin satırlık belgede 60 fps kaydırma (sanallaştırma)
- [ ] T0.6.5 Satır içi özel widget: SVG formül, onay kutusu, katlama oku
- [ ] T0.6.6 Erişilebilirlik API'si durumu (AccessKit)
- [ ] T0.6.7 Pano, sürükle bırak, dosya diyalogları
- [ ] T0.6.8 Spike raporu; go/no-go; D3 kapatılır (no-go ise Tauri + ProseMirror planı yazılır)

### 0.7 Matematik render prototipi (§9.2, D4)

- [ ] T0.7.1 mitex + typst + typst-svg prototipi; yüz formüllük korpus
- [ ] T0.7.2 ReX (veya güncel fork) prototipi, aynı korpus
- [ ] T0.7.3 Kapsam, kalite, binary boyutu, render süresi karşılaştırma tablosu
- [ ] T0.7.4 D4 kapatılır

### Faz 0 çıkış ölçütleri

- [ ] Korpusta yüzde yüz round-trip
- [ ] Emacs diferansiyelde en az yüzde 99 yapı eşitliği
- [ ] D1, D2, D3, D4 karara bağlandı
- [ ] `org-syntax` 0.1 crates.io'da

---

## Faz 1: MVP, "Org için Typora" (§20, 4 ila 6 ay)

### 1.1 org-model (§6.1)

- [ ] T1.1.1 Outline görünümü: başlık ağacı, seviye, aralıklar, kimlikler
- [ ] T1.1.2 TodoState: belge dizisi, done tespiti, birden fazla dizi
- [ ] T1.1.3 Tags: doğrudan, kalıtım, `#+FILETAGS`
- [ ] T1.1.4 Properties: drawer, `#+PROPERTY`, kalıtım, `_ALL`
- [ ] T1.1.5 Timestamps: tarih ayrıştırma (D13 kütüphane), tekrar kuralları, uyarı süreleri
- [ ] T1.1.6 Links: tür çözümleme, hedef aralıkları; Names haritası (`#+NAME`, target, CUSTOM_ID, ID)
- [ ] T1.1.7 Footnotes eşlemesi; Statistics çerez hesabı; Clock toplamları
- [ ] T1.1.8 Tembel önbellek ve CST kimliğiyle geçersiz kılma
- [ ] T1.1.9 Sorgular: `headlines_with_tag`, `scheduled_between`, `find_by_id`, `find_by_name`
- [ ] T1.1.10 Birim testleri (kalıtım, tekrar, çerez tabloları)

### 1.2 org-edit (§6.2)

- [ ] T1.2.1 `Transaction` tipi; çakışma kontrolü; uygulama ve ters çevirme
- [ ] T1.2.2 Geri al ve yinele yığını; 300 ms gruplama; imleç geri yükleme; `undo(redo(x)) == x` özellik testi
- [ ] T1.2.3 Stil çıkarımı: girinti biçimi, boş satır kuralı, `#+` harf biçimi, TODO dizisi
- [ ] T1.2.4 Başlık işlemleri: yükselt, alçalt, taşı, kes, kopyala, yapıştır, sırala
- [ ] T1.2.5 TODO döngüsü ve seçimi; öncelik; `CLOSED` ve logdone
- [ ] T1.2.6 Etiket ekleme ve kaldırma; dışlayan gruplar
- [ ] T1.2.7 Liste işlemleri: girinti, tür değiştirme, onay kutusu ve çerez güncelleme, yeniden numaralandırma
- [ ] T1.2.8 Vurgu sarma ve kaldırma; iç içe kuralları
- [ ] T1.2.9 Ekleme komutları: bağlantı, kaynak bloğu, zaman damgası (temel), yatay çizgi
- [ ] T1.2.10 Temel tablo işlemleri: satır ve sütun ekleme, silme, taşıma, hizalama (Emacs `org-table-align` birebir)
- [ ] T1.2.11 Daralt ve genişlet
- [ ] T1.2.12 Her komut için önce ve sonra snapshot testleri

### 1.3 kalem-core (§4, §11.2, §11.3, §14)

- [ ] T1.3.1 Editör durumu: rope, parse, model, seçim, belge meta
- [ ] T1.3.2 Komut kaydı: `Command`, `CommandHandler::Native`, kimlik kuralı, kategori
- [ ] T1.3.3 When-clause ayrıştırıcı ve değerlendirici
- [ ] T1.3.4 Tuş haritası: JSON yükleme, akor desteği, profil (word, org), çakışma raporu
- [ ] T1.3.5 Olay yolu: §11.3 olayları (veto ve zaman aşımı altyapısı, betik tarafı Faz 3)
- [ ] T1.3.6 Ayarlar: katmanlı yükleme (`settings.toml`, çalışma klasörü, belge anahtar kelimeleri)
- [ ] T1.3.7 Dosya işlemleri: aç, atomik kaydet, `.bak`, satır sonu ve BOM koruma, dış değişiklik algılama (`notify`)
- [ ] T1.3.8 Günlükleme (`tracing`) ve tanılama dosyası

### 1.4 kalem-ui (§7)

- [ ] T1.4.1 Uygulama iskeleti: pencere, menü çubuğu, araç çubuğu, durum çubuğu, tema altyapısı
- [ ] T1.4.2 Editör görünümü: sanallaştırılmış blok listesi, blok → CST eşlemesi
- [ ] T1.4.3 Blok render: headline (yıldız gizli, seviye stili, TODO, öncelik, etiket, katlama, çerez)
- [ ] T1.4.4 Blok render: paragraph ve satır içi run'lar; gizli işaret modeli (§6.3)
- [ ] T1.4.5 Blok render: liste, onay kutusu (tıklanabilir)
- [ ] T1.4.6 Blok render: tablo grid; hücre düzenleme; Tab gezinme
- [ ] T1.4.7 Blok render: src ve example blokları (sözdizimi vurgusu, kopyala düğmesi), quote, center, horizontal rule, keyword bloğu (`#+TITLE` büyük başlık)
- [ ] T1.4.8 Satır içi: bağlantı (tıklama, Ctrl/Cmd+tık), zaman damgası rozeti (salt görüntü), entity, çerez, line break
- [ ] T1.4.9 İmleç, seçim, IME, grafem hareketi, çift ve üçlü tık
- [ ] T1.4.10 Otomatik biçim tetikleyicileri (§6.3): `- `, `* `, `| `, `#+`, `[[`
- [ ] T1.4.11 Enter davranışları; Tab katlama ve girinti bağlamı
- [ ] T1.4.12 Yapıştırma: düz metin; HTML → Org dönüştürücü (basit); TSV → tablo
- [ ] T1.4.13 Kaynak görünümü: düz metin editörü, Org vurgulama, aynı rope ve geri alma
- [ ] T1.4.14 Bölünmüş görünüm
- [ ] T1.4.15 Taslak kenar çubuğu: ağaç, atlama, sürükle bırak
- [ ] T1.4.16 Komut paleti; bul ve değiştir çubuğu (regex seçeneği)
- [ ] T1.4.17 Durum çubuğu: kelime sayısı (belge ve alt ağaç), imleç, kaydetme durumu
- [ ] T1.4.18 Tarih seçici (temel), etiket tamamlama açılır kutusu
- [ ] T1.4.19 Ayarlar penceresi (yazı tipi, boyut, tema, profil)
- [ ] T1.4.20 gpui test harness ile widget testleri; manuel sürüm kontrol listesi

### 1.5 Yerelleştirme ve tema (§7.4, §7.5)

- [ ] T1.5.1 `fluent` altyapısı; İngilizce ve Türkçe dizgiler
- [ ] T1.5.2 Açık ve koyu tema TOML; sistem izleme
- [ ] T1.5.3 Yazı tipi seçimi; okunabilir satır genişliği; odak modu

### 1.6 Sürüm 0.1 (§17)

- [ ] T1.6.1 `cargo-dist` yapılandırması; üç platform için binary
- [ ] T1.6.2 macOS `.app` paketi (imzasız ilk sürüm kabul edilebilir; imzalama Faz 2)
- [ ] T1.6.3 Kullanıcı kılavuzu ilk sürüm (`docs/`, .org formatında)
- [ ] T1.6.4 Erken erişim: P1 ve P4 kişiliklerinden en az on kullanıcı; geri bildirim formu
- [ ] T1.6.5 Org geliştirici listesi ve ilgili topluluklara duyuru

### Faz 1 çıkış ölçütleri

- [ ] Org Manual kaynağı açılıp düzenlenip diff üretmeden kaydediliyor
- [ ] Soğuk açılış ve tuş gecikmesi hedefleri (§15) ölçülüp tutuyor
- [ ] On dış kullanıcıdan geri bildirim toplandı

---

## Faz 2: Belge yazarı (§20, 4 ay)

### 2.1 org-table (§8)

- [ ] T2.1.1 TBLFM ayrıştırıcı: sol taraf biçimleri, bayraklar, çoklu formül
- [ ] T2.1.2 İfade parser'ı (Pratt): aritmetik, referanslar, aralıklar, uzak referans, sabitler, parametreler
- [ ] T2.1.3 Fonksiyon kütüphanesi (§8.2 listesi); süre aritmetiği
- [ ] T2.1.4 Ondalık hesap (`rust_decimal`), hassasiyet ve biçim bayrakları
- [ ] T2.1.5 Bağımlılık grafiği, döngü tespiti, yineleme sınırı
- [ ] T2.1.6 Elisp formülü tespiti: koru, hesaplama, uyarı
- [ ] T2.1.7 Emacs'ta hesaplanmış tablo korpusu; birebir sonuç testi
- [ ] T2.1.8 UI: formül çubuğu, `#ERROR` gösterimi, referans vurgulama
- [ ] T2.1.9 Sıralama, CSV ve TSV içe ve dışa aktarma
- [ ] T2.1.10 `kalem fmt` tablo hizalama tamamlanır

### 2.2 org-math (§9.2)

- [ ] T2.2.1 D4 seçilen motorun crate entegrasyonu; font gömme
- [ ] T2.2.2 LaTeX fragment ve environment render; önbellek
- [ ] T2.2.3 `#+LATEX_HEADER` `\newcommand` alt kümesi
- [ ] T2.2.4 Hata gösterimi (kaynak + kırmızı çerçeve)
- [ ] T2.2.5 UI: `$` tetikleyici, tıklayınca kaynak düzenleme, önizleme geçişi
- [ ] T2.2.6 Görüntü snapshot testleri

### 2.3 org-export (§10)

- [ ] T2.3.1 `ExportTree` ve transkoder çatısı; filtre noktaları
- [ ] T2.3.2 Ortak davranış: `#+OPTIONS`, `:noexport:`, `EXCLUDE_TAGS`, `SELECT_TAGS`, makrolar, `#+INCLUDE`, `#+SETUPFILE`, alt ağaç dışa aktarma
- [ ] T2.3.3 HTML backend (ox-html sınıfları, CSS teması, formül SVG veya MathJax)
- [ ] T2.3.4 LaTeX backend (ox-latex davranışı, `#+LATEX_CLASS`, `ATTR_LATEX`, `%% org:LINE` yorumları)
- [ ] T2.3.5 Markdown backend (GFM)
- [ ] T2.3.6 Düz metin backend
- [ ] T2.3.7 PDF üretimi: `latexmk` / `xelatex` algılama; tectonic isteğe bağlı indirme (D5); hata eşleme
- [ ] T2.3.8 Pandoc köprüsü: DOCX, ODT, EPUB, RTF çıkış; DOCX, ODT, MD, HTML içe aktarma; "Org temizleme" geçişi; pandoc algılama ve yönlendirme
- [ ] T2.3.9 Dışa aktarma diyaloğu ve ayarları; `#+EXPORT_FILE_NAME`
- [ ] T2.3.10 Snapshot testleri; ox.el çıktısıyla karşılaştırma korpusu
- [ ] T2.3.11 `kalem export` CLI alt komutu

### 2.4 org-cite (§9.4)

- [ ] T2.4.1 org-cite sözdizimi ayrıştırma (stil, varyant, önek ve sonek, çoklu anahtar)
- [ ] T2.4.2 `hayagriva` ile BibTeX okuma; `#+bibliography`
- [ ] T2.4.3 CSL render (HTML, düz metin); `#+cite_export`
- [ ] T2.4.4 LaTeX'te biblatex ve natbib devri
- [ ] T2.4.5 UI: atıf ekleme diyaloğu (anahtar arama), hover önizleme

### 2.5 Editör özellikleri (§2.2, §3.2 Faz 2 sütunu)

- [ ] T2.5.1 Resimler: satır içi görüntüleme, `#+ATTR_ORG: :width`, yapıştırma ve sürükleme → `_assets/`, org-attach uyumu
- [ ] T2.5.2 Dipnotlar: ekleme, yeniden numaralandırma, belge sonu listesi, hover
- [ ] T2.5.3 Planning satırları: SCHEDULED, DEADLINE, CLOSED düzenleme, tarih seçici, tekrar kuralı
- [ ] T2.5.4 Property drawer: anahtar-değer tablosu düzenleme
- [ ] T2.5.5 Genel drawer, verse, export block, comment block render ve düzenleme
- [ ] T2.5.6 İçindekiler canlı önizleme (`#+TOC`)
- [ ] T2.5.7 Affiliated keyword'ler: `#+CAPTION`, `#+NAME` düzenleme; çapraz referans ekleme
- [ ] T2.5.8 Makro, export snippet, target ve radio target gösterimi
- [ ] T2.5.9 Sekmeler (D12): çoklu belge
- [ ] T2.5.10 Arşivleme ve refile (tek dosya içi)
- [ ] T2.5.11 Yazdırma: PDF üret ve sistem yazdırma diyaloğu

### 2.6 Pano (§10.3)

- [ ] T2.6.1 "Zengin metin olarak kopyala": seçim → HTML pano
- [ ] T2.6.2 HTML yapıştırma dönüştürücüsünü genişlet (tablolar, listeler, bağlantılar, vurgular)

### 2.7 Kitap yazımı doğrulaması (§9.4)

- [ ] T2.7.1 Örnek kitap bölümü: şekil, tablo, formül, atıf, dipnot, çapraz referans, `#+INCLUDE`
- [ ] T2.7.2 LaTeX ve PDF'e hatasız çıkış; HTML ve DOCX'e çıkış
- [ ] T2.7.3 Kelime sayısı hedefleri, bölüm istatistikleri

### 2.8 Dağıtım (§17)

- [ ] T2.8.1 macOS imzalama ve notarizasyon; Homebrew cask
- [ ] T2.8.2 Windows MSI ve imzalama
- [ ] T2.8.3 Linux AppImage ve Flatpak
- [ ] T2.8.4 Sürüm 0.2

### Faz 2 çıkış ölçütleri

- [ ] Kitap bölümü LaTeX ve PDF'e hatasız çıkıyor
- [ ] Tablo korpusu Emacs ile aynı hesaplıyor
- [ ] Üç platformda imzalı paket

---

## Faz 3: Genişletilebilirlik ve görevler (§20, 4 ay)

### 3.1 kalem-script (§11)

- [ ] T3.1.1 `rquickjs` entegrasyonu; quickjs-ng sürümü; `std` ve `os` modülleri kapalı
- [ ] T3.1.2 `ScriptHost` trait'i; QuickJS uygulaması
- [ ] T3.1.3 API tanım kaynağı (D6): Rust'ta tek tanım; `kalem.d.ts` üretimi; Lua açıklama üretimi için genişleme noktası
- [ ] T3.1.4 `kalem` ad alanı: `command`, `run`, `keymap`, `on`
- [ ] T3.1.5 `kalem.ui`: notify, prompt, confirm, quickPick, statusBar, panel (JSON widget ağacı, D11)
- [ ] T3.1.6 `kalem.settings`, `kalem.fs` (izinli), `kalem.net` (izinli)
- [ ] T3.1.7 `editor` ad alanı ve `Document`, `Headline`, `Table`, `Selection` arayüzleri
- [ ] T3.1.8 `kalem.exporter` (backend ve filtre kaydı), `kalem.tables.registerFunction`, `kalem.babel.registerLanguage`
- [ ] T3.1.9 Olay bağlamaları; veto ve zaman aşımı (500 ms)
- [ ] T3.1.10 İzin modeli: manifest bildirimi, ilk çalıştırma onayı, kapsamlar, `plugins.toml` kaydı
- [ ] T3.1.11 Zaman limiti (interrupt handler, 100 ms) ve bellek limiti (64 MB)
- [ ] T3.1.12 Eklenti yükleyici: manifest, etkinleştirme olayları, `activate` ve `deactivate`, Disposable toplama, ES modül çözümleme (yalnızca eklenti klasörü)
- [ ] T3.1.13 Hata yalıtımı: eklenti konsolu, tekrarlayan hatada devre dışı bırakma
- [ ] T3.1.14 `init.js` ve `keymap.json` yükleme
- [ ] T3.1.15 Worker eklentileri (ayrı runtime, mesajlaşma)
- [ ] T3.1.16 API sözleşme testleri; d.ts ile bağlama tutarlılık testi; limit testleri

### 3.2 Canlı çalışma zamanı (§11.9)

- [ ] T3.2.1 JS konsolu paneli: REPL, tamamlama, geçmiş
- [ ] T3.2.2 Sıcak yeniden yükleme: `init.js` ve eklenti dosyaları
- [ ] T3.2.3 `kalem --debug-socket` ve `kalem repl`; yalnızca localhost, varsayılan kapalı
- [ ] T3.2.4 `kalem.inspect.*`: tree, commands, timings
- [ ] T3.2.5 Soket üzerinden uçtan uca test sürücüsü

### 3.3 Eklenti ekosistemi (§11.5, §11.8)

- [ ] T3.3.1 Eklenti şablon deposu (TypeScript, esbuild, d.ts)
- [ ] T3.3.2 Örnek eklentiler: kelime sayısı, Pomodoro, özel dışa aktarma filtresi, tablo fonksiyonu
- [ ] T3.3.3 `kalem plugin install <url>` ve `kalem plugin list`
- [ ] T3.3.4 Eklenti API belgeleri (mdBook bölümü); "QuickJS tarayıcı değildir" sayfası
- [ ] T3.3.5 Güvenlik politikası (`SECURITY.md`)

### 3.4 org-babel (§12)

- [ ] T3.4.1 Başlık argümanı ayrıştırma (`:results`, `:exports`, `:var`, `:dir`, `:cache`, `:tangle`, `:file`)
- [ ] T3.4.2 Yürütücü arayüzü; alt süreç yönetimi; iptal; ilerleme
- [ ] T3.4.3 Diller: shell, python, javascript (node ve uygulama içi QuickJS), R, gnuplot, sqlite, org
- [ ] T3.4.4 Güven modeli: belge onayı, yol ve hash'e bağlı güven, asla otomatik çalıştırma
- [ ] T3.4.5 `#+RESULTS:` ekleme kuralları; `#+NAME` eşleşmesi; replace/append/prepend
- [ ] T3.4.6 `#+CALL:` ve inline src
- [ ] T3.4.7 Tangle: onay listesi, dosya yazma
- [ ] T3.4.8 UI: çalıştır düğmesi, sonuç bloğu render, gnuplot görüntüsü
- [ ] T3.4.9 Sahte yürütücü birim testleri; gerçek yorumlayıcı entegrasyon testleri (CI'da isteğe bağlı)

### 3.5 org-agenda (§13)

- [ ] T3.5.1 Çalışma klasörü kavramı; `.kalem/settings.toml`
- [ ] T3.5.2 İndeksleyici: arka plan tarama, `notify` ile güncelleme; bellek içi indeks (D8)
- [ ] T3.5.3 Günlük ve haftalık ajanda görünümü: SCHEDULED, DEADLINE, aktif zaman damgaları, tekrarlar, uyarı süreleri
- [ ] T3.5.4 TODO listesi; etiket ve özellik eşleme sözdizimi alt kümesi; tam metin arama
- [ ] T3.5.5 Görünümden eylemler: atlama, TODO değiştirme, yeniden zamanlama (sürükle bırak)
- [ ] T3.5.6 Capture şablonları (TOML) ve hızlı not penceresi
- [ ] T3.5.7 Refile: klasör içi başlık seçici
- [ ] T3.5.8 Saat: clock in ve out, `:LOGBOOK:`, toplamlar, çalışan saat göstergesi
- [ ] T3.5.9 `id:` bağlantı çözümleme (klasör geneli)

### 3.6 Diğer (§2.2, §10.1)

- [ ] T3.6.1 Yazım denetimi: `spellbook`, Hunspell sözlük yükleme, Türkçe ve İngilizce, satır içi işaretleme
- [ ] T3.6.2 reveal.js backend
- [ ] T3.6.3 Beamer backend
- [ ] T3.6.4 Şablon seçici (`#+SETUPFILE` kütüphanesi)
- [ ] T3.6.5 Sürüm 0.3

### Faz 3 çıkış ölçütleri

- [ ] En az üç topluluk eklentisi
- [ ] Agenda günlük kullanımda (kendi kullanımımız dahil)
- [ ] Babel ile gnuplot grafiği belgede üretilip dışa aktarılıyor

---

## Faz 4: Olgunlaşma (§20)

### 4.1 Betik ve eklenti katmanları

- [ ] T4.1.1 Lua ikinci betik dili (D10): `mlua`, `ScriptHost` uygulaması, aynı API, Lua açıklama üretimi
- [ ] T4.1.2 WASM eklentileri (`extism`): ağır işler, çok dilli
- [ ] T4.1.3 Dış süreç protokolü: JSON-RPC üzerinden stdio; Python örnek eklentisi
- [ ] T4.1.4 Eklenti indeksi (JSON) ve uygulama içi tarayıcı; API sürüm uyumluluğu

### 4.2 Org kapsamı

- [ ] T4.2.1 Babel `:session` (kalıcı REPL) ve `:noweb`
- [ ] T4.2.2 Sütun görünümü (`#+COLUMNS`)
- [ ] T4.2.3 org-habit çizelgesi
- [ ] T4.2.4 Diary sexp zaman damgalarının agenda'da hesaplanması
- [ ] T4.2.5 Inlinetask tam render; dynamic block güncelleme (`#+BEGIN: clocktable`)
- [ ] T4.2.6 org-crypt eklentisi (topluluk)

### 4.3 Ürün

- [ ] T4.3.1 Sunum modu: üst düzey başlık = slayt, tam ekran, salt okunur
- [ ] T4.3.2 Gömülü PDF önizleme paneli (pdfium)
- [ ] T4.3.3 Otomatik güncelleme
- [ ] T4.3.4 Erişilebilirlik: ekran okuyucu temel desteği; klavyeyle tam gezinme
- [ ] T4.3.5 Performans ayarı: §15 tablosunun tamamı; bellek profili
- [ ] T4.3.6 Daktilo modu; ek temalar
- [ ] T4.3.7 Sürüm 1.0

---

## Sürekli işler (her fazda)

- [ ] TS.1 Tasarım belgesinin İngilizce çevirisi (yayın öncesi); sonrasında iki dilin senkronu
- [ ] TS.2 `CHANGELOG.md` her PR'da güncellenir
- [ ] TS.3 Benchmark gerilemesi CI'da PR'ı engeller
- [ ] TS.4 Korpus büyütme ve lisans kaydı
- [ ] TS.5 Bilinen farklar belgesinin güncel tutulması; Org Syntax belirsizliklerinin üst akıma raporlanması
- [ ] TS.6 Kullanıcı kılavuzu ve eklenti API belgeleri (mdBook, .org kaynak, Kalem ile dışa aktarma)
- [ ] TS.7 Bağımlılık güncellemeleri; gpui ve rquickjs sürüm takibi
- [ ] TS.8 Topluluk: iyi ilk konular, PR incelemeleri, sürüm notları

---

## Açık kararlar takibi (§21)

| ID | Karar | Nerede kapanır | Durum |
|---|---|---|---|
| D1 | Lisans | T0.1.2 | Açık |
| D2 | Parser temeli | T0.2.6 | Açık |
| D3 | UI çerçevesi | T0.6.8 | Açık |
| D4 | Matematik motoru | T0.7.4 | Açık |
| D5 | tectonic dağıtımı | T2.3.7 | Açık |
| D6 | API tanım kaynağı | T3.1.3 | Açık |
| D7 | Proje adı | T0.1.1 (uygunluk kontrolü) | **Kalem** seçildi |
| D8 | Agenda indeks depolama | T3.5.2 | Açık |
| D9 | Yapılandırma formatları | T1.3.6 | Açık |
| D10 | Lua desteği zamanı | T4.1.1 | Açık |
| D11 | Eklenti panellerinde webview | T3.1.5 | Açık |
| D12 | Çoklu belge | T2.5.9 | Açık |
| D13 | Zaman kütüphanesi | T1.1.5 | Açık |

---

## Reddedilen ve ertelenen fikirler (§4.6)

- [-] Elixir/BEAM ana motor, Rust NIF; Elixir eklenti dili. Gerekçe §4.6. Canlı çalışma zamanı ihtiyacı T3.2 ile karşılanır.
- [-] Gömülü Python eklenti dili. Python, Babel (T3.4.3) ve dış süreç eklentileri (T4.1.3) ile.
- [~] Tauri + ProseMirror birincil arayüz: yalnızca T0.6.8 no-go derse.
