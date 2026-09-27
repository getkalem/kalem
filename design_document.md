# Kalem: Org Mode Tabanlı WYSIWYG Belge Editörü — Tasarım Belgesi

| Alan | Değer |
|---|---|
| Sürüm | 0.1 (taslak) |
| Tarih | 2026-09-27 |
| Durum | Tartışmaya açık, kod yazılmadan önce hazırlandı |
| Dil | Türkçe. Yayın öncesi İngilizce çevirisi yapılacak (bkz. todo.md) |
| İlgili | todo.md (bu belgeden türetilmiş iş listesi) |

## İçindekiler

0. Bu belge hakkında
1. Vizyon ve kapsam
2. Ürün tanımı
3. Org formatı desteği
4. Mimari
5. Parser: org-syntax
6. Belge modeli ve düzenleme: org-model, org-edit
7. Arayüz: kalem-ui
8. Tablo motoru: org-table
9. LaTeX ve matematik
10. Dışa ve içe aktarma
11. Eklenti sistemi
12. Babel: kod blokları
13. Agenda ve çalışma klasörü
14. Ayarlar ve yapılandırma
15. Performans hedefleri
16. Test stratejisi
17. Paketleme ve dağıtım
18. Açık kaynak ve topluluk
19. Riskler ve azaltma
20. Yol haritası
21. Açık kararlar
22. Sözlük
23. Kaynaklar

---

## 0. Bu belge hakkında

Bu belge uygulamanın ne olduğunu, ne olmadığını, nasıl inşa edileceğini ve hangi sırayla inşa edileceğini tanımlar. Kararlar değiştikçe güncellenir. Kesinleşmemiş kararlar 21. bölümde numaralı (D1, D2, ...) tutulur; karar verildiğinde ilgili bölüme taşınır ve 21. bölümde "karar verildi" olarak işaretlenir.

Okuyucu: proje sahibi, ileride katkıda bulunacak geliştiriciler, eklenti yazarları.

Bu belgede **ZORUNLU**, **ÖNERİLİR**, **İSTEĞE BAĞLI** ifadeleri RFC 2119 anlamında kullanılır.

Uygulamanın adı **Kalem**'dir (D7 karara bağlandı). Kütüphane crate'leri `org-*`, uygulama crate'leri `kalem-*` ön ekiyle anılır. Binary adı `kalem`.

---

## 1. Vizyon ve kapsam

### 1.1 Tek cümle

Kalem, Emacs bilmeyen insanların Org dosyalarını Word benzeri bir arayüzle yazıp düzenleyebildiği; hafif, hızlı, tek binary, açık kaynak bir masaüstü editörü. Kısaca: **Org için Typora**.

### 1.2 Problem

- Org formatı düz metin belge, taslak, görev ve tablo için en olgun formatlardan biridir; ancak pratikte Emacs'a kilitlidir.
- Emacs'ın öğrenme eğrisi, formatın kendisinden yararlanmak isteyen çoğu insanı dışarıda bırakır.
- Mevcut alternatifler eksiktir: Organice (web, sınırlı), Orgzly ve beorg (mobil, görev odaklı), Logseq (Org ikinci sınıf), VS Code eklentileri (kaynak görünümü, WYSIWYG yok). Masaüstünde tam WYSIWYG Org editörü yoktur.
- Office ve Electron tabanlı not uygulamaları ağırdır, düz metin değildir ya da kapalı formattadır.

### 1.3 Hedefler

| ID | Hedef |
|---|---|
| G1 | **Kayıpsız.** Emacs'ta oluşturulmuş bir dosya açılıp kaydedildiğinde dokunulmayan her byte aynı kalır. |
| G2 | **Emacs'sız kullanılabilir.** Kullanıcı Org sözdizimini hiç görmeden belge, liste, tablo, görev ve formül yazabilir. |
| G3 | **Hafif ve hızlı.** Tek binary. Soğuk açılış 300 ms altı, 10 MB dosya 1 s altı, tuş gecikmesi 16 ms altı. |
| G4 | **Org çekirdeği yerleşik.** Bölüm 3.2'deki öğe tablosu, faz planına göre. |
| G5 | **LaTeX.** Satır içi matematik önizleme ve LaTeX/PDF dışa aktarma ile kitap ve makale yazımı. |
| G6 | **Genişletilebilir.** Komut kaydı, olaylar, JavaScript eklentileri; ileride Lua ve WASM. |
| G7 | **Emacs ile birlikte yaşar.** Aynı dosya iki uygulamada dönüşümlü düzenlenebilir, diff gürültüsü olmaz. |
| G8 | **Yeniden kullanılabilir.** Parser ve dışa aktarma bağımsız crate olarak yayınlanır. |

### 1.4 Hedef olmayanlar

- **Microsoft Office uyumluluğu.** docx yerel format değildir. İçe ve dışa aktarma pandoc üzerinden sağlanır.
- **Sayfa düzeni editörü.** Kenar boşluğu, sütun, sayfa sonu, sayfa numarası yerleşimi. Org anlamsal formattır; görünüm dışa aktarmada belirlenir.
- **Tam hesap tablosu.** Pivot, grafik motoru, yüz binlerce satır.
- **Görsel slayt tasarımcısı.** Sunum yalnızca dışa aktarma ve basit sunum modu olarak.
- **Emacs'ı taklit etmek.** Elisp, Emacs tuş dili, agenda'nın her ayarı.
- **Gerçek zamanlı işbirliği ve bulut senkron.** Git ve dosya senkronu yeterli kabul edilir.
- **Mobil platformlar.**
- **Org'un yüzde yüzü ilk sürümde.** Kapsam fazlara bölünür.

### 1.5 Hedef kullanıcılar

| ID | Kişilik | İhtiyaç |
|---|---|---|
| P1 | Ortak yazar | Emacs kullanan bir yazarla aynı .org dosyasını düzenlemek zorunda; Emacs öğrenmek istemiyor. |
| P2 | Düz metin arayan bilgi çalışanı | Obsidian veya Notion'dan kaçıyor; görev, not ve belge tek yerde, dosyalar kendi diskinde. |
| P3 | Akademisyen, kitap yazarı | LaTeX çıktısı, atıf, formül, uzun belge, bölüm dosyaları. |
| P4 | Eski Emacs kullanıcısı | Yıllardır biriken .org dosyaları var, Emacs'ı bırakmış. |
| P5 | Eklenti geliştiricisi | JS/TS biliyor, Obsidian veya VS Code eklentisi yazmış; benzer bir API bekliyor. |

### 1.6 Başarı ölçütleri

- Round-trip test korpusunda yüzde yüz byte eşitliği.
- Org Manual'ın .org kaynağı açılıp düzenlenebiliyor ve kaydedildiğinde diff üretmiyor.
- Emacs org-element ile diferansiyel testte yapı eşitliği en az yüzde 99; bilinen farklar belgelenmiş.
- Bölüm 15'teki performans hedefleri ölçülüp tutturulmuş.
- 1.0 sürümünde: üç platformda paketlenmiş, en az beş topluluk eklentisi, en az bir Emacs kullanıcısı ortak yazarıyla gerçek kullanım.

---

## 2. Ürün tanımı

### 2.1 Temel deneyim

- **WYSIWYG görünüm varsayılan.** Typora modeli: işaretleyiciler (`*`, `/`, `=`, `[[ ]]`, `#+`) gizlidir; imleç öğenin içine girince görünür, çıkınca tekrar gizlenir.
- **Kaynak görünümü** tek tuşla açılır. Aynı belge, aynı imleç konumu, aynı geri alma geçmişi.
- **Bölünmüş görünüm** isteğe bağlı: solda kaynak, sağda WYSIWYG.
- **Taslak (outline) kenar çubuğu:** başlık ağacı, tıklayınca atlama, sürükle bırak ile taşıma.
- **Araç çubuğu:** kalın, italik, altı çizili, üstü çizili, kod, başlık seviyesi, liste türleri, onay kutusu, tablo, bağlantı, resim, dipnot, TODO, etiket, tarih, formül.
- **Komut paleti** (Ctrl/Cmd+Shift+P): her komuta isimle erişim.
- **Katlanabilir başlıklar:** Tab ile döngüsel katlama, Emacs ile aynı davranış.
- **Durum çubuğu:** kelime sayısı, imleç konumu, kaydetme durumu, belge dili, çalışan arka plan işleri.

### 2.2 Office karşılıkları

Kullanıcının Word, Excel ve PowerPoint'ten beklediği özelliklerin Org karşılıkları ve uygulamadaki durumu.

| Kullanıcı beklentisi | Org karşılığı | Uygulamada | Faz |
|---|---|---|---|
| Kalın, italik, altı çizili, üstü çizili, kod | `*b*` `/i/` `_u_` `+s+` `~c~` `=v=` | Araç çubuğu, kısayol | 1 |
| Başlık stilleri | `*` `**` `***` | Başlık seviyesi seçici, Ctrl+1..6 | 1 |
| Madde ve numaralı liste | `-` `+` `1.` `1)` | Araç çubuğu, otomatik biçim | 1 |
| Onay kutusu | `- [ ]` `[X]` `[-]` | Tıklanabilir kutu, istatistik `[2/5]` | 1 |
| Tablo | `\| a \| b \|` | Grid düzenleme, Tab gezinme | 1 |
| Tablo formülü | `#+TBLFM:` | Formül çubuğu, otomatik hesap | 2 |
| Resim | `[[file:x.png]]` | Satır içi görüntüleme, yapıştırma | 2 |
| Dipnot | `[fn:1]` | Ekleme, yeniden numaralandırma | 2 |
| İçindekiler | `#+TOC:` veya dışa aktarma | Canlı önizleme | 2 |
| Bağlantı | `[[url][açıklama]]` | Ctrl+K, tıklama | 1 |
| Formül | `$x^2$` `\begin{equation}` | Canlı önizleme | 2 |
| Yorum | `# ...` veya `:COMMENT:` | Soluk gösterim | 2 |
| Değişiklik izleme | yok | Hedef değil; git önerilir | – |
| Yazım denetimi | yok (Emacs flyspell) | spellbook + Hunspell sözlükleri | 3 |
| Kelime sayısı | yok | Durum çubuğu, alt ağaç bazlı | 1 |
| Yazdırma | Dışa aktarma → PDF | PDF üret ve sistem yazdırma | 2 |
| Şablon, stil | `#+SETUPFILE`, dışa aktarma sınıfları | Şablon seçici | 3 |
| Hesap tablosu | Tablo + `#+TBLFM` | Formül çubuğu, referanslar | 2 |
| Sıralama | `org-table-sort-lines` | Sütun başlığından sıralama | 2 |
| CSV içe ve dışa aktarma | `org-table-import` / `export` | Menü | 2 |
| Grafik | Babel + gnuplot | Kod bloğu çalıştırma | 3 |
| Sunum | reveal.js, Beamer dışa aktarma | Dışa aktarma | 3 |
| Sunum modu | yok | Uygulama içi basit tam ekran sunum | 4 |

### 2.3 Dosya modeli

- **Belge tek .org dosyasıdır.** UTF-8. Satır sonu dosyadan alınır (LF veya CRLF), korunur. BOM korunur.
- **Ekler yan dosyadır.** Org ikili veri gömmez. Resimler ve ekler belgeye göreli yolla bağlanır. Yapıştırılan veya sürüklenen resim `<belge-adı>_assets/` klasörüne yazılır ve göreli bağlantı eklenir; klasör adı ayarlanabilir. org-attach'ın `data/` düzenine uyumluluk sağlanır (`:ATTACH_DIR:` ve `attachment:` bağlantıları çözümlenir).
- **Çalışma klasörü isteğe bağlıdır.** Agenda, çoklu dosya araması ve `id:` bağlantı çözümlemesi için.
- **Kaydetme atomiktir.** Geçici dosyaya yaz, sonra yeniden adlandır. İsteğe bağlı `.bak`.
- **Dış değişiklik algılanır.** Dosya izleyici ile; belge değişmemişse sessizce yeniden yüklenir, değişmişse kullanıcıya sorulur.
- **Şifreleme hedef değildir.** org-crypt ileride eklenti olarak.

### 2.4 Platformlar

macOS 12+, Linux (X11 ve Wayland), Windows 10+. Tek binary, kurulum gerektirmez. Paketleme bölüm 17'de.

### 2.5 Emacs ile birlikte yaşama

- Belge içi ayarlar (`#+TODO`, `#+TAGS`, `#+STARTUP`, `#+PROPERTY`) uygulanır. Uygulama kendi ayarlarını belgeye yazmaz; kullanıcı açıkça isterse yazar.
- Yeni üretilen sözdizimi belgenin mevcut stilini izler: girinti biçimi, boş satır kuralları, `#+` anahtar kelimelerin büyük veya küçük harf yazımı, TODO anahtar kelime dizisi.
- Tablo hizalaması Emacs'ın `org-table-align` davranışıyla birebir aynıdır; aksi halde her kaydetmede tablolar diff üretir.
- Dosya kilidi kullanılmaz.

---

## 3. Org formatı desteği

### 3.1 Referanslar

| Kaynak | Rol |
|---|---|
| Org Syntax (Worg) | Normatif kabul edilir |
| `org-element.el` | Tartışmalı durumlarda davranış referansı |
| Org Manual | Anlambilim ve kullanıcıya görünen davranış |
| Hedef sürüm | Org 9.7 davranışı; sürüm farkları belgelenir |

### 3.2 Öğe kapsamı

Sütunlar: Parse (kayıpsız ayrıştırma), Render (WYSIWYG gösterim), Düzenle (yapısal düzenleme desteği). Parser ilk sürümden itibaren her öğeyi tanımak **ZORUNLU**dur; tanımadığı yapıyı paragraf metni olarak korur. Render ve düzenleme faz planına göre gelir.

**Büyük öğeler (greater elements)**

| Öğe | Parse | Render | Düzenle |
|---|---|---|---|
| Headline ve section | 0 | 1 | 1 |
| Planning satırı (SCHEDULED, DEADLINE, CLOSED) | 0 | 1 | 2 |
| Property drawer | 0 | 1 | 2 |
| Genel drawer | 0 | 1 | 2 |
| Plain list (sırasız, sıralı, tanım) | 0 | 1 | 1 |
| Item ve checkbox | 0 | 1 | 1 |
| Table (org) | 0 | 1 | 1 |
| Table (table.el) | 0 | 2 (kaynak) | – |
| Footnote definition | 0 | 2 | 2 |
| Greater block (center, quote, special) | 0 | 1 | 2 |
| Dynamic block | 0 | 2 | 3 |
| Inlinetask | 0 | 3 | 3 |

**Küçük öğeler (lesser elements)**

| Öğe | Parse | Render | Düzenle |
|---|---|---|---|
| Paragraph | 0 | 1 | 1 |
| Src block | 0 | 1 | 1 |
| Example block | 0 | 1 | 1 |
| Export block | 0 | 2 | 2 |
| Verse block | 0 | 2 | 2 |
| Comment block | 0 | 2 | 2 |
| Fixed-width (`: `) | 0 | 1 | 2 |
| Horizontal rule | 0 | 1 | 1 |
| Keyword (`#+...`) | 0 | 1 | 2 |
| Affiliated keyword (CAPTION, NAME, ATTR_*, HEADER) | 0 | 2 | 2 |
| Babel call (`#+CALL:`) | 0 | 3 | 3 |
| Clock | 0 | 2 | 3 |
| Diary sexp | 0 | 2 (kaynak) | – |
| LaTeX environment | 0 | 2 | 2 |
| Node property | 0 | 1 | 2 |
| Comment (`# `) | 0 | 1 | 2 |
| Table row, table cell | 0 | 1 | 1 |

**Nesneler (objects)**

| Nesne | Parse | Render | Düzenle |
|---|---|---|---|
| Bold, italic, underline, strike-through | 0 | 1 | 1 |
| Code, verbatim | 0 | 1 | 1 |
| Link (file, http/https, id, custom-id, fuzzy, radio, coderef, `#+LINK` kısaltmaları, attachment) | 0 | 1 | 1 |
| Plain link, angle link | 0 | 1 | 1 |
| Timestamp (aktif, pasif, aralık, tekrar, uyarı) | 0 | 1 | 2 |
| Footnote reference (adlı, satır içi, anonim) | 0 | 2 | 2 |
| Inline src block, inline babel call | 0 | 2 | 3 |
| LaTeX fragment | 0 | 2 | 2 |
| Entity (`\alpha`) | 0 | 1 | 2 |
| Subscript, superscript | 0 | 1 | 2 |
| Line break (`\\`) | 0 | 1 | 1 |
| Macro (`{{{x}}}`) | 0 | 2 | 3 |
| Export snippet (`@@html:...@@`) | 0 | 2 | 2 |
| Target (`<<x>>`), radio target (`<<<x>>>`) | 0 | 2 | 2 |
| Statistics cookie (`[1/3]`, `[50%]`) | 0 | 1 | 1 |
| Citation (org-cite `[cite:@key]`) | 0 | 2 | 2 |

### 3.3 Round-trip garantisi

Biçimsel tanım: her `text` girdisi için `parse(text).to_string() == text` **ZORUNLU**dur. Bu, fuzz ve korpus testleriyle sürekli doğrulanır.

Düzenlemeler metin aralıklarını değiştirir; dokunulmayan aralıklar byte düzeyinde aynı kalır. Belgeyi yeniden biçimlendirmek yalnızca açık bir kullanıcı komutuyla olur ("Belgeyi yeniden biçimlendir").

Tek istisna tablolardır: Org'un kendisi tabloyu düzenleyince yeniden hizalar. Uygulama da yalnızca kullanıcının düzenlediği tabloyu, Emacs ile aynı algoritmayla hizalar.

### 3.4 Belge içi yapılandırma

Aşağıdaki anahtar kelimeler bir ön geçişte okunur ve ayrıştırmayı, gösterimi veya dışa aktarmayı etkiler:

| Anahtar kelime | Etkisi |
|---|---|
| `#+TODO`, `#+SEQ_TODO`, `#+TYP_TODO` | Başlıklarda TODO anahtar kelimeleri; birden fazla dizi; `(t@/!)` günlük işaretleri |
| `#+TAGS` | Etiket tamamlama, karşılıklı dışlayan gruplar `{ }` |
| `#+STARTUP` | overview/content/showall, indent, hidestars, logdone, fold davranışı vb. |
| `#+PROPERTY` | Belge geneli özellikler ve kalıtım |
| `#+PRIORITIES` | Öncelik aralığı |
| `#+FILETAGS` | Dosya etiketleri |
| `#+LINK` | Bağlantı kısaltmaları |
| `#+MACRO` | Makro tanımları |
| `#+CONSTANTS` | Tablo formülü sabitleri |
| `#+SETUPFILE`, `#+INCLUDE` | Harici dosyadan ayar ve içerik |
| `#+OPTIONS`, `#+TITLE`, `#+AUTHOR`, `#+DATE`, `#+LANGUAGE`, `#+EXPORT_FILE_NAME`, `#+EXCLUDE_TAGS`, `#+SELECT_TAGS` | Dışa aktarma |
| `#+LATEX_CLASS`, `#+LATEX_HEADER`, `#+HTML_HEAD`, `#+CITE_EXPORT`, `#+BIBLIOGRAPHY` | Backend'e özel dışa aktarma |
| `#+ARCHIVE`, `#+CATEGORY`, `#+COLUMNS` | Arşiv, agenda, sütun görünümü |
| `#+TBLFM` | Tablo formülleri (tablo düzeyinde) |

`#+SETUPFILE` yüklemesi dosya sistemi soyutlamasıyla enjekte edilir; testlerde sahte yükleyici kullanılır.

### 3.5 Bilinçli kısıtlar

- Elisp içeren `#+TBLFM` formülleri (`'(...)`) korunur, hesaplanmaz, uyarı gösterilir.
- Diary sexp zaman damgaları korunur; agenda'da ilk sürümde hesaplanmaz.
- table.el tabloları korunur, düzenlenmez.
- org-crypt, org-columns görünümü, org-habit çizelgesi ilk sürümlerde yoktur.
- Inlinetask basit render edilir.

---

## 4. Mimari

### 4.1 İlkeler

1. **Metin gerçek kaynaktır.** CST metnin kayıpsız görünümüdür; model CST'den türetilir. Akış tek yönlüdür: metin → CST → model → görünüm. Düzenlemeler metne uygulanır, asla modele.
2. **Çekirdek arayüzden bağımsızdır.** `org-*` crate'leri GUI bağımlılığı içermez; komut satırından ve testlerden kullanılabilir.
3. **Her kullanıcı eylemi bir komuttur.** Menü, kısayol, palet ve eklentiler aynı komut kaydından geçer.
4. **Belirlenimci ve test edilebilir.** Zaman, dosya sistemi ve rastgelelik enjekte edilir.
5. **Hafiflik bir özelliktir.** Her yeni bağımlılık binary boyutu ve başlangıç süresi etkisiyle gerekçelendirilir.

### 4.2 Crate haritası

Cargo workspace düzeni:

```
crates/
  org-syntax/     Kayıpsız CST, lexer ve parser, artımlı yeniden ayrıştırma
  org-model/      Anlamsal katman: başlık ağacı, TODO, etiket, zaman, özellik sorguları
  org-edit/       Düzenleme transaksiyonları, geri al/yinele, yapısal komutlar
  org-table/      Tablo hizalama, TBLFM ayrıştırma ve hesaplama
  org-math/       LaTeX matematik → vektör görüntü (önizleme)
  org-export/     HTML, LaTeX, Markdown, reveal.js, Beamer; pandoc köprüsü
  org-cite/       org-cite ayrıştırma, hayagriva ile CSL ve BibTeX
  org-babel/      Kod bloğu çalıştırma (alt süreç), sonuç ekleme, tangle
  org-agenda/     Çalışma klasörü indeksi, agenda sorguları
  kalem-cli/      Komut satırı aracı: parse, check, fmt, export, diff-emacs
  kalem-core/     Editör durumu, komut kaydı, tuş haritası, ayarlar, olay yolu
  kalem-script/   QuickJS ev sahibi, API bağlamaları, d.ts üretimi, eklenti yükleyici
  kalem-ui/       gpui arayüzü: editör görünümü, taslak, paneller
  kalem/          Ana binary
tests/
  corpus/         Gerçek dünya .org dosyaları (lisanslı)
  emacs/          Diferansiyel test için Elisp betikleri
docs/             Kullanıcı kılavuzu, eklenti API belgeleri (.org formatında)
```

Bağımlılık kuralları:

- `org-*` crate'leri asla `kalem-*` crate'lerine bağımlı olmaz.
- `kalem-ui`, `org-*` crate'lerini doğrudan okuyabilir ama her değişikliği `kalem-core` komutlarıyla yapar.
- `kalem-script` yalnızca `kalem-core` API'sine bağlanır; `org-*` tiplerini doğrudan görmez.

### 4.3 Katmanlar ve veri akışı

```
Kullanıcı girdisi (klavye, fare, IME)
        │
        ▼
   kalem-ui (gpui) ──── komut çağrısı ────▶ kalem-core: Command Registry ◀──── kalem-script (QuickJS)
        ▲                                          │
        │ render                                   ▼
        │                              org-edit: Transaction { edits: [(Range, String)] }
        │                                          │
        │                                          ▼
        │                        Rope (metin) ── artımlı parse ──▶ org-syntax CST
        │                                                              │
        │                                                              ▼
        └──────────── görünüm modeli ◀── org-model (türetilmiş, tembel) ◀┘
```

- Metin `ropey` rope'unda tutulur. CST byte ofsetleriyle konuşur.
- Bir düzenleme: rope güncellenir → etkilenen bölümler yeniden ayrıştırılır → model cache'i geçersiz kılınır → arayüz yalnızca değişen blokları yeniden çizer.

### 4.4 İş parçacığı modeli

| İş parçacığı | Görev |
|---|---|
| UI | gpui olay döngüsü, render, komut yürütme (kısa süreli) |
| Parser | Senkron ve artımlı, UI'da çalışır. Tuş başına bütçe 2 ms; aşarsa arka plana atılır, eski ağaçla render edilir. İlk açılışta büyük dosyalar arka planda ayrıştırılır. |
| Script | QuickJS UI iş parçacığında çalışır; interrupt handler ile 100 ms senkron bütçe. Ağır işler için ayrı QuickJS runtime'lı worker eklentileri, mesajla haberleşir. |
| Worker havuzu | Dışa aktarma, matematik render, indeksleme, yazım denetimi, resim yükleme |
| Alt süreçler | Babel, pandoc, tectonic/latexmk |

### 4.5 Temel bağımlılıklar

| Alan | Crate | Not |
|---|---|---|
| CST | rowan | rust-analyzer'ın kayıpsız ağaç kütüphanesi |
| Metin | ropey | Rope |
| Unicode | unicode-segmentation, unicode-width | Grafem ve genişlik |
| GUI | gpui | Zed'in çerçevesi; D3 |
| JS | rquickjs | quickjs-ng tabanlı; sürüm kontrol edilecek |
| Regex | regex | |
| Serileştirme | serde, toml, serde_json | |
| Matematik | typst + mitex, alternatif ReX | D4 |
| Atıf | hayagriva | BibTeX ve CSL |
| Yazım | spellbook | Helix'in kullandığı Hunspell uyumlu motor |
| Dosya izleme | notify | |
| Zaman | jiff veya chrono | Zaman damgaları ve tekrarlar |
| Ondalık | rust_decimal | Tablo hesapları |
| Günlük | tracing | |
| Test | insta, proptest, cargo-fuzz, criterion | |
| Yerelleştirme | fluent | |
| Dağıtım | cargo-dist | |

Harici ve isteğe bağlı araçlar: pandoc, tectonic veya latexmk, python ve diğer Babel yorumlayıcıları, gnuplot.

### 4.6 Değerlendirilip reddedilen mimari alternatifler

**Elixir/BEAM ana motor, Rust NIF olarak.** Fikir: BEAM uygulamanın "işletim sistemi" olur; parser ve yerleşim gibi ağır işler Rustler NIF'leriyle Rust'ta yapılır; eklenti dili doğal olarak Elixir olur; çalışan uygulamaya `iex` ile bağlanıp canlı inceleme ve test yapılır. **Reddedildi:**

- **G3 ile çelişir.** BEAM çalışma zamanı paketlenmek zorundadır (20 MB ve üzeri). Burrito ile tek dosya yapılsa bile ilk çalıştırmada diske açılır. Soğuk açılış ve bellek tabanı hedefleri tutmaz.
- **Elixir'in yerel masaüstü GUI'si yok.** Scenic zengin metin düzenleme için yetersiz, `:wx` eski. LiveView + webview yolu editörü JavaScript'e taşır ve Rust'ı UI döngüsünden çıkarır. gpui ana iş parçacığını ve olay döngüsünü kendisi ister; BEAM başka bir sürece gömülemez. Sonuç iki ayrı süreç ve aralarında IPC olur. Tuş → ekran yolunda BEAM hiçbir katkı sağlamaz, yalnızca gecikme ve karmaşıklık ekler.
- **Güçlü olduğu yerde değiliz.** BEAM çok istemcili, uzun ömürlü sunucular için tasarlandı. Tek kullanıcı, tek belge, tek UI iş parçacığı olan bir editörde süpervizör ağacının ve binlerce hafif sürecin doğal karşılığı yoktur.
- **Elixir eklentileri sandbox'lanamaz.** Her modül `File` ve `System`'e erişir; bölüm 11.6'daki izin modeli kurulamaz. Hedef eklenti yazarı kitlesi (P5) Elixir bilmez.
- **İki dil, iki derleme sistemi.** mix ve cargo; katkı için Erlang/OTP, Elixir, Rust ve Zig kurulumu.

Fikrin doğru tarafı: Emacs'ın asıl gücü canlı, incelenebilir, sıcak yeniden yüklenebilir bir çalışma zamanı olmasıdır ve BEAM bunun en yakın modern karşılığıdır. Bu ihtiyaç Kalem'de QuickJS katmanıyla karşılanır (bkz. 11.9): uygulama içi JS konsolu, `init.js` ve eklentilerin yeniden başlatmadan yüklenmesi, çalışan uygulamaya soket üzerinden bağlanan hata ayıklama REPL'i. Elixir, ileride çok kullanıcılı bir sunucu ürünü (işbirliği, web) düşünülürse orada doğru araçtır.

**Gömülü Python eklenti dili.** Reddedildi: boyut, sandbox yokluğu, GIL ve dağıtım yükü nedeniyle (bölüm 11). Python, Babel kod blokları ve dış süreç eklentileri üzerinden desteklenir.

**Electron veya Tauri birincil arayüz.** Ertelendi, reddedilmedi: D3 spike'ı başarısız olursa Tauri + ProseMirror yedek yoldur.

---

## 5. Parser: org-syntax

### 5.1 Gereksinimler

| ID | Gereksinim |
|---|---|
| R1 | Kayıpsız: her token korunur, `to_string()` girdiyle aynıdır |
| R2 | Hata toleranslı: hiçbir girdi panic üretmez; bozuk yapılar paragraf olarak korunur |
| R3 | Artımlı: tipik düzenleme 2 ms altında yeniden ayrıştırılır |
| R4 | Bağlamsal: `#+TODO` gibi anahtar kelimeler başlık ayrıştırmasını etkiler |
| R5 | Hızlı: tam ayrıştırma en az 10 MB/s |
| R6 | Byte ofsetli; satır ve sütun dönüşümü rope üzerinden |

### 5.2 Yaklaşım

- **rowan** GreenNode ve SyntaxNode. `SyntaxKind` enum'u hem token türlerini (WHITESPACE, NEWLINE, STAR, TEXT, ...) hem düğüm türlerini (DOCUMENT, SECTION, HEADLINE, PARAGRAPH, ...) içerir.
- **İki aşama**, org-element ile aynı: (1) eleman düzeyi, satır tabanlı: başlık, blok, liste, tablo, çekmece, anahtar kelime, paragraf; (2) nesne düzeyi, paragraf, başlık ve hücre içi: vurgu, bağlantı, zaman damgası, dipnot, formül.
- **Ön geçiş:** belge başı anahtar kelimeleri ve `#+SETUPFILE` içeriği okunur; `ParseContext { todo_keywords, tags, link_abbrevs, macros, startup, constants }` üretilir.
- **Nesne kuralları** Org'un regex'leriyle birebir: vurgu için önce ve sonra karakter kısıtları ve en fazla iki satır sonu; bağlantı türleri; zaman damgası biçimleri.

### 5.3 Artımlı yeniden ayrıştırma

- Granülerlik bölümdür (başlık + içeriği). Düzenleme hangi bölümlere değdiyse onlar yeniden ayrıştırılır.
- Yıldızlı satır eklenmesi veya silinmesi bölüm sınırlarını değiştirir; komşu bölümler de yeniden ayrıştırılır.
- Blok sınırları (`#+BEGIN` / `#+END`) değişince kapsayan bölüm tamamen yeniden ayrıştırılır.
- Ön geçiş anahtar kelimeleri değişirse belge tamamen yeniden ayrıştırılır.
- rowan'ın green tree'si değişmeyen alt ağaçları paylaşır.

### 5.4 Hata toleransı

- Kapanmamış blok paragraf olur; org-element de böyle davranır.
- Bilinmeyen `#+` anahtar kelimesi KEYWORD düğümü olarak korunur.
- Bozuk zaman damgası düz metin olur.
- Fuzz testi hiçbir girdinin panic üretmediğini doğrular.

### 5.5 API taslağı

```rust
pub struct Parse { green: GreenNode, errors: Vec<SyntaxError> }

pub fn parse(text: &str, ctx: &ParseContext) -> Parse;
pub fn reparse(old: &Parse, edit: &TextEdit, ctx: &ParseContext) -> Parse;
pub fn context_from(text: &str, loader: &dyn SetupFileLoader) -> ParseContext;

/// Tipli AST sarmalayıcıları, rust-analyzer'ın `ast` katmanı gibi.
pub mod ast {
    pub struct Headline(SyntaxNode);
    impl Headline {
        pub fn level(&self) -> usize;
        pub fn todo_keyword(&self) -> Option<SyntaxToken>;
        pub fn priority(&self) -> Option<char>;
        pub fn title(&self) -> Option<Title>;
        pub fn tags(&self) -> impl Iterator<Item = SyntaxToken>;
        pub fn planning(&self) -> Option<Planning>;
        pub fn properties(&self) -> Option<PropertyDrawer>;
        pub fn section(&self) -> Option<Section>;
        pub fn children(&self) -> impl Iterator<Item = Headline>;
    }
}
```

### 5.6 orgize değerlendirmesi (D2)

`orgize` crate'i 0.10 sürümünden itibaren rowan tabanlıdır. Sıfırdan yazmadan önce bir haftalık değerlendirme **ÖNERİLİR**:

- Kayıpsızlık: korpus üzerinde round-trip testi.
- Kapsam: 3.2'deki tabloya karşı eksik öğeler.
- Artımlı yeniden ayrıştırma: var mı, eklenebilir mi.
- Bakım durumu, yanıt süresi, lisans (MIT).
- API uygunluğu: tipli AST katmanı, hata raporlama.

Sonuç üç yoldan biri: doğrudan bağımlılık, fork, sıfırdan yazım. Fork ve sıfırdan yazımda 5.2'deki tasarım geçerlidir.

### 5.7 Test

- Round-trip fuzz (`cargo-fuzz`): `parse(x).to_string() == x` ve panic yok.
- Snapshot testleri (`insta`): her öğe için ağaç çıktısı.
- **Emacs diferansiyel testi:** `tests/emacs/dump.el` betiği `org-element-parse-buffer` sonucunu JSON'a döker (tür, begin, end, temel özellikler). Rust tarafı aynı JSON'u üretir. Normalize edilip karşılaştırılır. CI'da Emacs kurulu bir container çalışır. Örnek çağrı:

```bash
emacs --batch -l org -l tests/emacs/dump.el --eval '(dump-org-json "tests/corpus/manual.org")'
```

- Korpus: Org Manual .org kaynağı, Worg sayfaları, izinli topluluk dosyaları, sentetik uç durumlar.
- Benchmark (`criterion`): 1 MB ve 10 MB dosya tam parse; tipik düzenleme artımlı parse.

---

## 6. Belge modeli ve düzenleme: org-model, org-edit

### 6.1 org-model

CST'den türetilen, tembel hesaplanan ve önbelleklenen görünümler:

| Görünüm | İçerik |
|---|---|
| Outline | Başlık ağacı, seviye, ID, aralıklar |
| TodoState | Başlığın TODO durumu, belgenin dizisindeki konumu, "done" olup olmadığı |
| Tags | Doğrudan ve kalıtımla gelen etiketler, `#+FILETAGS` |
| Properties | `:PROPERTIES:` ve `#+PROPERTY` kalıtımı, `_ALL` sözdizimi |
| Timestamps | Ayrıştırılmış tarihler, tekrar kuralları (`+1w`, `++1m`, `.+1d`), uyarı süreleri |
| Links | Türü çözümlenmiş bağlantılar, hedef aralıkları |
| Footnotes | Tanım ve referans eşlemesi |
| Names | `#+NAME`, `<<target>>`, `CUSTOM_ID`, `ID` haritası (çapraz referans) |
| Statistics | Onay kutusu ve TODO istatistik çerezleri, hesaplanmış |
| Clock | CLOCK satırları, toplam süreler |

Önbellek CST düğüm kimliğiyle geçersiz kılınır. Sorgular: `headlines_with_tag`, `scheduled_between`, `find_by_id`, `find_by_name`.

### 6.2 org-edit

```rust
pub struct Transaction {
    pub edits: Vec<(TextRange, String)>,   // çakışmayan, sıralı
    pub selection_after: Option<Selection>,
    pub label: String,                      // geri alma menüsü için
}
```

- **Geri al ve yinele:** transaksiyon yığını; ters düzenlemeler saklanır; yazma işlemleri 300 ms içinde gruplanır; imleç konumu geri yüklenir.
- **Yapısal işlemler** (her biri bir komut, transaksiyon üretir):
  - Başlık: yükselt, alçalt (alt ağaçla birlikte), yukarı ve aşağı taşı, alt ağacı kes, kopyala ve yapıştır, arşivle (`org-archive` uyumlu: ayrı dosya veya `:ARCHIVE:` başlığı altı), refile, sırala.
  - TODO: döngü (belgenin dizisiyle), doğrudan seç, öncelik artır ve azalt, `CLOSED` ekleme ve günlük (`#+STARTUP: logdone`, `LOGBOOK` notları).
  - Etiket: ekle, kaldır, tamamlama, karşılıklı dışlayan gruplar.
  - Zamanlama: SCHEDULED ve DEADLINE ayarla, tarih seçici, tekrar kuralı.
  - Saat: clock in, clock out, `:LOGBOOK:` altına CLOCK satırı.
  - Liste: girinti artır ve azalt, tür değiştir, onay kutusu değiştir ve istatistik güncelle, yeniden numaralandır.
  - Tablo: bölüm 8.
  - Ekleme: bağlantı, dipnot (yeniden numaralandırma), kaynak bloğu, zaman damgası, resim, makro, entity.
  - Vurgu: seçimi sar, işaretleri kaldır, iç içe vurgu kurallarına uyum.
  - Daralt ve genişlet (narrow/widen): tek alt ağaçta çalışma.
- **Stil çıkarımı:** belgenin girinti biçimi (org-indent ya da düz), başlık sonrası boş satır kuralı, TODO dizisi, `#+` anahtar kelime harf biçimi belgeden okunur. Yeni içerik bu stille üretilir.
- **Konumlar:** her şey byte ofsetidir. Arayüz satır ve sütun dönüşümünü rope üzerinden yapar. İmleç hareketi grafem kümeleri üzerinden.

### 6.3 WYSIWYG düzenleme anlambilimi

- **Gizli işaret modeli:** bir öğenin işaret token'ları yalnızca imleç öğe içindeyken görünür. Gizli token'lar imleç hareketinde atlanır; seçim ve silme işlemleri gizli token'ları öğe bütünlüğüne göre ele alır.
- **Yazarken vurgu:** Ctrl+B seçim varsa sarar, yoksa "kalın modu" açar; yazılan metin işaretler arasına gider.
- **Otomatik biçim:** satır başında `- ` liste, `* ` başlık, `| ` tablo, `#+` tamamlama menüsü, `[[` bağlantı menüsü, `$` formül önizleme, `[fn:` dipnot menüsü.
- **Enter davranışı:** listede yeni madde, boş maddede listeden çıkış, tabloda alt satır, başlık satırında yeni paragraf, kod bloğunda düz satır.
- **Yapıştırma:** HTML panodan Org'a dönüşüm; düz metin olduğu gibi; resim dosya olarak kaydedilip bağlantılanır; TSV tabloya.
- **Tehlikeli düzenlemeler:** bir işaret token'ını bozacak silme (örneğin `*kalın*`'daki yıldızı silmek) öğeyi düz metne çevirir ve kullanıcıya gösterir; asla sessizce yapı kaybı olmaz.

---

## 7. Arayüz: kalem-ui

### 7.1 Framework kararı (D3)

Karar: **gpui**. Gerekçe: saf Rust, Zed ile metin editörü için kanıtlanmış, GPU hızlandırmalı, tek binary. Risk: API henüz hareketli, belgeler seyrek, düzenleme motoru yeniden kullanılabilir bir bileşen değil; sıfırdan yazılır.

Bu risk üç haftalık bir **spike** ile ölçülür. Spike hedefleri:

1. Rope destekli, satır içi stilli düzenlenebilir paragraflar.
2. IME kompozisyonu macOS, Windows ve Linux'ta.
3. Yüz bin satırlık belgede 60 fps kaydırma (sanallaştırma).
4. Satır içi özel widget: SVG formül, onay kutusu, katlama oku.
5. Erişilebilirlik API'sinin durumu (AccessKit entegrasyonu var mı).
6. Pano, sürükle bırak, dosya diyalogları.

Go/no-go: 1, 2 ve 3 çalışmıyorsa **Tauri + ProseMirror** yedek yoluna geçilir. Çekirdek crate'ler değişmez; yalnızca `kalem-ui` yeniden yazılır.

### 7.2 Görünüm bileşenleri

**Editör görünümü:** sanallaştırılmış blok listesi. Her CST elemanı bir blok render'ıdır.

| Eleman | Gösterim |
|---|---|
| Headline | Yıldızlar gizli, seviyeye göre yazı boyutu, TODO rozeti, öncelik, etiketler sağa yaslı, katlama oku, istatistik çerezi |
| Paragraph | Satır içi run'lar |
| Plain list | Girinti, madde işareti ya da numara, onay kutusu |
| Table | Grid, sütun hizalama, seçili hücre vurgusu |
| Src block | Sözdizimi vurgulu kod, dil etiketi, "çalıştır" ve "kopyala" düğmeleri |
| Example, fixed-width | Mono blok |
| Quote, center, verse | Stilize blok |
| LaTeX environment | Render edilmiş formül; tıklayınca kaynak |
| Image link | Satır içi resim, boyut `#+ATTR_ORG: :width` ile |
| Drawer | Katlanabilir; `:PROPERTIES:` için anahtar-değer tablosu |
| Keyword | `#+TITLE` büyük başlık; `#+AUTHOR`, `#+DATE` alt bilgi; diğerleri soluk ve katlanmış "belge ayarları" bloğu |
| Footnote definition | Belge sonunda numaralı liste |
| Horizontal rule | Çizgi |
| Comment | Soluk, isteğe bağlı gizli |

**Satır içi nesneler:** vurgular, bağlantı (tıklanabilir, Ctrl/Cmd+tık), zaman damgası (rozet, tıklayınca takvim), dipnot referansı (üst simge, hover'da içerik), formül (render), entity (sembol), istatistik çerezi, atıf (biçimlendirilmiş), hedef ve radyo hedef (soluk çapa).

**Paneller:** taslak kenar çubuğu, özellikler paneli (sağ), agenda paneli, bul ve değiştir çubuğu, komut paleti, durum çubuğu, eklenti konsolu.

**Kaynak görünümü:** düz metin editörü, Org sözdizimi vurgulama, aynı rope ve aynı geri alma yığını.

### 7.3 Klavye

- **Varsayılan profil, Word benzeri:** Ctrl+B/I/U, Ctrl+1..6 başlık seviyesi, Ctrl+Shift+L liste, Tab ve Shift+Tab girinti ya da katlama (bağlama göre), Ctrl+Enter TODO döngüsü, Ctrl+K bağlantı, Ctrl+Shift+T tablo, Alt+ok alt ağacı taşı.
- **Org profili (isteğe bağlı):** Emacs Org tuşları; `C-c C-t`, `C-c C-s`, `M-RET`, `M-ok` vb.
- Tuş haritası JSON'da tutulur, komut kaydına bağlanır, when-clause destekler (`editorFocus && inTable`).

### 7.4 IME, dil, erişilebilirlik

- IME kompozisyon katmanı: CJK ve ölü tuşlar için **ZORUNLU**.
- Arayüz dili: fluent ile İngilizce ve Türkçe; topluluk çevirileri.
- Erişilebilirlik: gpui'nin AccessKit durumu spike'ta ölçülür. Hedef: 1.0'da ekran okuyucu için temel destek (blok yapısı, başlık düzeyleri, düzenleme).
- Sağdan sola diller: ilk sürümde hedef değil; parser ve rope RTL'e engel olmamalı.

### 7.5 Görsel tasarım

- Tema: açık ve koyu, sistemi izler. Temalar TOML dosyasında. Kullanıcı CSS'i yoktur.
- Yazı tipleri: gövde için serif ya da sans seçilebilir, kod için mono, formüller için matematik fontu gömülü.
- Okunabilir satır genişliği (ayarlanabilir), odak modu (yalnızca geçerli alt ağaç), daktilo modu (isteğe bağlı).

---

## 8. Tablo motoru: org-table

### 8.1 Kapsam

- Org tabloları: satırlar, `|---|` yatay çizgiler, hücre hizalama (sayı sağa, metin sola), `<r>` `<l>` `<c>` hizalama işaretleri, `<N>` sütun genişliği ve daraltma.
- Düzenleme: hücre içi metin, Tab ve Shift+Tab, Enter alt satır, satır ve sütun ekleme, silme, taşıma, yatay çizgi ekleme, sıralama (alfabetik, sayısal, zaman), hizalama.
- Hizalama Emacs'ın `org-table-align` davranışıyla birebir aynı **ZORUNLU** (bkz. 2.5 ve 3.3).
- table.el tabloları kapsam dışıdır.

### 8.2 TBLFM

**Dilbilgisi:** `#+TBLFM: LHS=RHS;flags::LHS=RHS;flags...`

**Sol taraf:** `$3` (sütun formülü), `@2$3` (alan formülü), `@>$2`, `$<`, `$>`, `$name` (adlandırılmış sütun, `!` satırı), `@I..@II$3` (yatay çizgi referansları).

**Sağ taraf:**
- Aritmetik: `+ - * / ^ %`, parantez.
- Referanslar: `$1`, `@2`, `@2$3`, `@-1`, `@+1`, `@<`, `@>`, `$<`, `$>`, aralıklar `@2$1..@5$1`, `$1..$3`, `@I..@II`.
- Uzak referans: `remote(TABLO_ADI, @2$1)`.
- Sabitler: `$PI`, `$e`, `#+CONSTANTS`.
- Parametreler: `^` ve `_` satırlarından `$name`.
- Fonksiyonlar: `vsum vmean vmax vmin vprod vcount vlen vmedian vsdev vvar`, `abs sqrt exp ln log sin cos tan floor ceil round trunc mod`, `if`, `min max`, temel string işlemleri, süre aritmetiği (HH:MM, HH:MM:SS).
- **Bayraklar:** `%.2f` (printf), `N` (sayı, boş sıfır), `E` (boş koru), `L` (literal), `t` `T` `U` (süre), `f3` (basamak), `p10` (hassasiyet), `s` (bilimsel), `e` (mühendislik).
- **Elisp formülleri** `'(...)`: korunur, hesaplanmaz, hücrede uyarı simgesi.

**Hesap motoru:**
- Kendi ifade parser'ı (Pratt). Ondalık aritmetik (`rust_decimal`) Emacs Calc'a yakın sonuçlar için; hassasiyet bayrakları uygulanır.
- Bağımlılık grafiği ile hesaplama sırası; döngü tespiti; "tümünü yeniden hesapla" en fazla 10 yineleme (Emacs `C-u C-u C-c *` eşleniği).
- Uyumluluk testi: Emacs'ta hesaplanmış tablo korpusu; aynı sonuçlar **ZORUNLU**.

### 8.3 Arayüz

- Formül çubuğu: seçili hücrenin ya da sütunun formülü; düzenleme `#+TBLFM` satırını günceller.
- Hata gösterimi: `#ERROR` hücresi ve açıklama.
- Referans vurgulama: formül düzenlenirken referans verilen hücreler renklenir.
- CSV ve TSV içe ve dışa aktarma; panodan TSV yapıştırınca tablo.

---

## 9. LaTeX ve matematik

### 9.1 Org içinde LaTeX

Parçalar: `$x$`, `$$...$$`, `\(...\)`, `\[...\]`, `\begin{env}...\end{env}` (equation, align, gather, matrix ve türevleri), `\alpha` gibi entity'ler, `#+BEGIN_EXPORT latex`, `#+LATEX:` satırları, `#+LATEX_HEADER`, `#+LATEX_HEADER_EXTRA`, `#+LATEX_CLASS`, `#+LATEX_CLASS_OPTIONS`, `#+ATTR_LATEX`.

### 9.2 Satır içi önizleme: org-math

- Girdi: LaTeX matematik alt kümesi, amsmath ve amssymb seviyesi. `#+LATEX_HEADER` içindeki `\newcommand` tanımları sınırlı ölçüde uygulanır.
- Çıktı: vektör (glif yolları ve çizgiler). gpui'de doğrudan çizilir. Önbellek: formül metni ve boyut hash'i → görüntü.
- Hata: render edilemeyen formül kaynak metin olarak kırmızı çerçeveyle gösterilir; asla kaybolmaz.

**Seçenekler (D4):**

| Seçenek | Artı | Eksi |
|---|---|---|
| A. mitex (LaTeX → Typst math) + typst kütüphanesi ile yerleşim + typst-svg | Üretim kalitesi yerleşim, aktif bakım, saf Rust | mitex'in LaTeX kapsamı; Typst'in font ve dünya kurulumu; birkaç MB font |
| B. ReX ve fork'ları (saf Rust TeX matematik yerleşimi, OpenType MATH) | Küçük, TeX algoritmaları | Bakım durumu belirsiz, kapsam eksik |
| C. KaTeX QuickJS içinde → HTML/MathML | Kapsam çok geniş | Yerel render yok; elenir |

Karar önerisi: A ile prototip. Yüz formüllük korpusla kapsam ölçülür. B yedek.

### 9.3 Tam LaTeX ve PDF

- `org-export` LaTeX backend'i (ox-latex davranışı) `.tex` üretir.
- Derleme: sistemde `latexmk` veya `xelatex` varsa kullanılır. Yoksa **tectonic** isteğe bağlı olarak indirilir (kullanıcı onayı, ağ erişimi). Gömme kararı D5; öneri ayrı indirilebilir yardımcı araç, çünkü tectonic binary boyutunu ciddi artırır ve paketleri ağdan çeker.
- PDF önizleme: ilk sürümde harici görüntüleyici; sonra gömülü panel (pdfium).
- Hata eşleme: LaTeX günlüğündeki satır numaraları üretilen `.tex`'e eklenen `%% org:LINE` yorumlarıyla Org konumuna çevrilir.

### 9.4 Kitap ve makale yazımı

- **Yapı:** `#+LATEX_CLASS: book`; başlık seviyeleri part, chapter, section olarak eşlenir; `#+INCLUDE:` ile bölüm dosyaları; `#+OPTIONS: toc:t num:t`.
- **Şekil ve tablo:** `#+CAPTION`, `#+NAME`, `#+ATTR_LATEX: :width :placement`; çapraz referans `[[fig:x]]` → `\ref`.
- **Atıf:** org-cite sözdizimi `[cite:@key]`, `[cite/t:@a;@b]`, `[cite:see @a p. 3]`; `#+bibliography: refs.bib`; `#+cite_export: csl apa.csl` ya da `biblatex` / `natbib`. `org-cite` crate'i ayrıştırır; `hayagriva` BibTeX okur ve CSL ile HTML ve düz metin üretir; LaTeX'te biblatex veya natbib'e devredilebilir.
- **Dizin:** `#+INDEX:` → `\index`.
- **Sözlük ve kısaltmalar:** eklentiyle.
- Dipnotlar, epigraf, verse blokları.
- **Yazar araçları:** bölüm bazlı kelime sayısı, hedef takibi, `:noexport:` ile taslak bölümler.
- EPUB ve DOCX: pandoc üzerinden.

---

## 10. Dışa ve içe aktarma

### 10.1 Yerleşik backend'ler

| Backend | Referans | Faz |
|---|---|---|
| HTML | ox-html sınıfları, tek dosya, CSS teması, MathJax ya da gömülü SVG | 2 |
| LaTeX | ox-latex | 2 |
| Markdown | ox-md, GFM tabloları | 2 |
| Düz metin | ox-ascii | 2 |
| reveal.js | org-re-reveal seçeneklerinin alt kümesi | 3 |
| Beamer | ox-beamer | 3 |

**Ortak davranış:** `#+OPTIONS` (toc, num, ^, _, *, ', -, todo, tags, pri, d, f, e, H, ...), `:noexport:` ve `:export:` etiketleri, `#+EXCLUDE_TAGS`, `#+SELECT_TAGS`, export snippet'leri, `#+BEGIN_EXPORT`, makrolar (`{{{title}}}`, `{{{date(FORMAT)}}}`, `{{{n}}}`, `{{{property(X)}}}`), `#+INCLUDE`, `#+SETUPFILE`, alt ağaç dışa aktarma, `EXPORT_FILE_NAME` özelliği, `#+EXPORT_SELECT_TAGS`.

**Mimari:** `org-model` → `ExportTree` → transkoder. ox.el deseni: her öğe türü için backend fonksiyonu, öncesi ve sonrası filtreler. Eklentiler JavaScript ile yeni backend ve filtre kaydedebilir.

### 10.2 Pandoc köprüsü

- Çıkış: DOCX, ODT, EPUB, RTF. İki yol: (a) `.org` dosyasını doğrudan pandoc'un Org okuyucusuna ver; (b) uygulamanın HTML çıktısını pandoc'a ver. Varsayılan (b), çünkü uygulamanın Org yorumu daha eksiksiz; kullanıcı (a)'yı seçebilir.
- İçe: DOCX, ODT, Markdown, HTML → Org (pandoc), ardından uygulamanın "Org temizleme" geçişi (boş satırlar, başlık biçimi, tablo hizalama).
- Pandoc yoksa: algılama, indirme bağlantısı, kısa yönerge.

### 10.3 Pano

- "Zengin metin olarak kopyala": seçim HTML olarak panoya; Word ve e-postaya yapıştırma.
- HTML yapıştırma → Org: dahili basit dönüştürücü, pandoc gerekmez.

---

## 11. Eklenti sistemi

### 11.1 Katmanlar

| Katman | Teknoloji | Amaç | Faz |
|---|---|---|---|
| Komut kaydı ve olaylar | Rust, `kalem-core` | Her şeyin temeli | 1 |
| Kullanıcı betiği | `init.js`, QuickJS | Kısayol, küçük komutlar, otomasyon | 3 |
| Eklenti paketi | JS/TS + manifest, QuickJS | Dağıtılabilir özellikler | 3 |
| İkinci betik dili | Lua (mlua), aynı API | Tercih edenler için (D10) | 4 |
| Ağır ve çok dilli eklenti | WASM (extism) | Hesaplama yoğun işler | 4 |
| Dış süreç | JSON-RPC üzerinden stdio | Python vb. entegrasyonlar | 4 |

Betik dilleri `ScriptHost` trait'i arkasında durur; API tanımı tek kaynaktan üretilir (D6). Böylece Lua eklenmesi yalnızca bağlama katmanıdır.

### 11.2 Komut kaydı

```rust
pub struct Command {
    pub id: String,                  // "org.todo.cycle", "table.insertRow", "<plugin>.<name>"
    pub title: String,               // palet ve menü için, yerelleştirilir
    pub category: String,
    pub default_keys: Vec<KeyChord>,
    pub when: Option<WhenClause>,    // "editorFocus && inTable"
    pub handler: CommandHandler,     // Rust fn veya betik callback'i
    pub args_schema: Option<JsonSchema>,
}

pub enum CommandHandler {
    Native(fn(&mut EditorContext, serde_json::Value) -> CommandResult),
    Script(ScriptCallbackId),
}
```

- Komutlar transaksiyon içinde çalışır; geri alma birimi komuttur.
- Eklenti komutları yerleşik komutlarla aynı palet, menü ve tuş haritasında görünür.
- ID kuralı: `alan.eylem`; eklentiler `pluginId.eylem`.

### 11.3 Olaylar

| Olay | Zaman | Veto |
|---|---|---|
| `app:ready` | Başlangıç tamamlandı | – |
| `document:open`, `document:close` | | – |
| `document:before-save` | Kaydetmeden önce | evet |
| `document:after-save` | | – |
| `document:changed` | Aralık bilgisiyle, debounce'lu | – |
| `selection:changed` | | – |
| `headline:todo-changed`, `headline:tags-changed`, `headline:scheduled` | | – |
| `table:before-recalc`, `table:recalculated` | | – |
| `babel:before-execute`, `babel:after-execute` | | evet |
| `export:before`, `export:after` | Filtre olarak; çıktıyı değiştirebilir | evet |
| `workspace:file-changed` | Dosya izleyici | – |

Veto edilebilir olaylar zaman aşımlıdır (500 ms); aşarsa olay geçer, uyarı yazılır.

### 11.4 JavaScript API yüzeyi

TypeScript tanımı (`kalem.d.ts`) üretilir ve eklenti şablonuyla dağıtılır. Taslak:

```ts
declare namespace kalem {
  const version: string;
  function command(id: string, spec: { title: string; run: (...args: unknown[]) => unknown | Promise<unknown>; when?: string; keys?: string[] }): Disposable;
  function run(id: string, ...args: unknown[]): Promise<unknown>;
  function keymap(keys: string, commandId: string, opts?: { when?: string }): Disposable;
  function on<E extends keyof Events>(event: E, handler: (e: Events[E]) => void | Promise<void>): Disposable;

  namespace ui {
    function notify(message: string, level?: "info" | "warn" | "error"): void;
    function prompt(title: string, opts?: { default?: string; placeholder?: string }): Promise<string | null>;
    function confirm(message: string): Promise<boolean>;
    function quickPick<T>(items: { label: string; detail?: string; value: T }[], opts?: { placeholder?: string }): Promise<T | null>;
    namespace statusBar { function set(id: string, text: string, opts?: { tooltip?: string; command?: string }): Disposable; }
    namespace panel { function register(id: string, spec: PanelSpec): Disposable; } // JSON widget ağacı, webview değil (D11)
  }
  namespace settings { function get<T>(key: string): T; function set(key: string, value: unknown): void; function onChange(key: string, cb: () => void): Disposable; }
  namespace fs  { function read(path: string): Promise<string>; function write(path: string, text: string): Promise<void>; function list(dir: string): Promise<string[]>; } // izinle
  namespace net { function fetch(url: string, init?: RequestInit): Promise<Response>; } // izinle
  namespace babel { function registerLanguage(name: string, runner: BabelRunner): Disposable; }
  namespace exporter { function registerBackend(name: string, backend: ExportBackend): Disposable; function addFilter(stage: string, fn: ExportFilter): Disposable; }
  namespace tables { function registerFunction(name: string, fn: (...args: number[]) => number): Disposable; }
}

declare namespace editor {
  const document: Document;
  const selection: Selection;
  function insert(text: string, at?: number): void;
  function replace(range: Range, text: string): void;
  function transact(label: string, fn: () => void): void;
}

interface Document {
  readonly path: string | null;
  text(range?: Range): string;
  headlines(): Headline[];
  headlineAt(offset: number): Headline | null;
  nodeAt(offset: number): Node;
  find(query: { tag?: string; todo?: string; property?: [string, string] }): Headline[];
  keywords(): Record<string, string[]>;
  save(): Promise<void>;
}

interface Headline {
  readonly level: number; title: string; todo: string | null; priority: string | null;
  tags: string[]; readonly properties: Record<string, string>;
  scheduled: Timestamp | null; deadline: Timestamp | null;
  readonly range: Range; readonly parent: Headline | null;
  children(): Headline[]; body(): string;
  setTodo(state: string | null): void; setTitle(title: string): void; setTags(tags: string[]): void;
  setProperty(key: string, value: string | null): void;
  promote(): void; demote(): void; moveUp(): void; moveDown(): void;
}

interface Table {
  readonly rows: number; readonly cols: number;
  cell(row: number, col: number): string; setCell(row: number, col: number, value: string): void;
  formulas(): string[]; recalc(): void;
}
```

### 11.5 Eklenti paketi

Manifest `plugin.json`:

```json
{
  "id": "com.example.wordcount",
  "name": "Word Count",
  "version": "0.1.0",
  "description": "Alt ağaç bazlı kelime sayısı",
  "main": "dist/main.js",
  "api": "^1.0",
  "activation": ["onStartup"],
  "permissions": ["fs:read:workspace"],
  "contributes": {
    "commands": [{ "id": "com.example.wordcount.show", "title": "Kelime sayısını göster" }],
    "keybindings": [{ "command": "com.example.wordcount.show", "keys": "ctrl+shift+w" }],
    "settings": [{ "key": "com.example.wordcount.includeDrawers", "type": "boolean", "default": false }]
  }
}
```

- Konum: platform standart yapılandırma dizini altında `plugins/<id>/`.
- Etkinleştirme olayları: `onStartup`, `onCommand:<id>`, `onLanguage:<babel-dili>`, `onDocument`.
- Yaşam döngüsü: `export function activate(ctx)` ve `deactivate()`. `ctx.subscriptions` ile Disposable'lar toplanır.
- Modül sistemi: ES modülleri. `import` yalnızca eklenti klasöründen. esbuild ile tek dosyaya paketleme önerilir; şablon depo sağlanır.
- QuickJS bir JS motorudur, tarayıcı değil: DOM yok, Node API'si yok, npm'in yerel modülleri yok. Bu, eklenti belgelerinde ilk sayfada yazar.

### 11.6 Güvenlik ve kaynak sınırları

- **Sandbox:** QuickJS'in `std` ve `os` modülleri yüklenmez. Yalnızca `kalem` ve `editor` nesneleri görünür.
- **İzinler** manifestte bildirilir; ilk çalıştırmada kullanıcıya gösterilir ve onaylanır. Kapsamlar: `fs:read:workspace`, `fs:write:workspace`, `fs:read:all`, `net:fetch:<alan-adı>`, `subprocess` (ayrı ve açık uyarı).
- **Zaman limiti:** interrupt handler; senkron çağrı 100 ms'yi aşarsa iptal edilir ve uyarı verilir. Uzun işler için async API ve worker eklentileri.
- **Bellek limiti:** runtime başına, varsayılan 64 MB.
- Eklenti hatası uygulamayı çökertmez; eklenti konsolunda gösterilir; tekrarlayan hatada eklenti devre dışı bırakılır.
- Belge içindeki kod (Babel) eklentilerden ayrı bir güven modelidir (bölüm 12).

### 11.7 Kullanıcı yapılandırması

| Dosya | İçerik |
|---|---|
| `settings.toml` | Statik ayarlar |
| `init.js` | Başlangıçta çalışan kişisel betik; komutlar, kısayollar |
| `keymap.json` | Tuş haritası geçersiz kılmaları |
| `plugins.toml` | Etkin eklentiler ve izin kararları |
| `themes/*.toml` | Kullanıcı temaları |

### 11.8 Dağıtım

- İlk sürüm: git URL'si ya da klasörden kurulum; `kalem plugin install <url>`.
- Sonra: topluluk indeksi (JSON), uygulama içi eklenti tarayıcısı, sürüm uyumluluğu kontrolü (`api` alanı).

### 11.9 Canlı çalışma zamanı

Emacs'ın "çalışırken içine girip değiştirme" deneyimi Kalem'de şu araçlarla sağlanır:

- **JS konsolu paneli:** uygulama içinde `kalem` ve `editor` API'sine erişen bir REPL; Emacs'ın `M-:` ve `*scratch*` karşılığı. Tamamlama ve geçmiş.
- **Sıcak yeniden yükleme:** `init.js` ve eklenti dosyaları değişince yeniden başlatma olmadan yeniden yüklenir; eski Disposable'lar temizlenir.
- **Hata ayıklama soketi:** `kalem --debug-socket` ile çalışan uygulamaya yerel Unix soketi veya TCP üzerinden bağlanıp JS değerlendirme; `kalem repl` komutu bağlanır. Yalnızca localhost, varsayılan kapalı.
- **İnceleme komutları:** `kalem.inspect.tree(offset)` CST'yi, `kalem.inspect.commands()` komut kaydını, `kalem.inspect.timings()` son işlemlerin sürelerini döker.
- **Test kancası:** aynı soket üzerinden uçtan uca testler çalışan uygulamayı sürer (aç, düzenle, kaydet, doğrula).

---

## 12. Babel: kod blokları

- **Sözdizimi:** `#+BEGIN_SRC lang :header args`, `#+CALL:`, `src_lang{...}`, `#+RESULTS:` blokları.
- **Başlık argümanları:** `:results` (output, value; raw, table, list, verbatim, file, drawer; replace, append, prepend, silent), `:exports` (code, results, both, none), `:var`, `:dir`, `:cache`, `:tangle`, `:file`; `:session` ve `:noweb` 4. fazda.
- **Yürütücüler:** shell (sh, bash, zsh), python, javascript (node ya da uygulama içi QuickJS), R, gnuplot, sqlite, org, calc benzeri basit aritmetik. Eklentiler `kalem.babel.registerLanguage` ile dil ekler.
- **Güven modeli:** belge ilk kez "çalıştır" isteğinde onay; "bu belgeye güven" kararı belge yoluna ve içerik hash'ine bağlıdır; hiçbir kod açılışta otomatik çalışmaz, `#+STARTUP` ile bile.
- **Sonuç ekleme:** Org kurallarıyla `#+RESULTS:` konumu, `#+NAME` ile eşleşme, eski sonucun değiştirilmesi.
- **Tangle:** dosyalara yazma, her yazma için onay listesi.
- Çalışan bloklar için ilerleme göstergesi ve iptal.

---

## 13. Agenda ve çalışma klasörü

- Çalışma klasörü açılınca `.org` dosyaları arka planda indekslenir; dosya izleyici ile güncel tutulur. İndeks bellek içi; büyük klasörler için diskte önbellek (D8).
- **Görünümler:** günlük ve haftalık ajanda (SCHEDULED, DEADLINE, aktif zaman damgaları, tekrarlar, uyarı süreleri, `CLOSED` ile gizleme), TODO listesi, etiket ve özellik araması (Org eşleme sözdiziminin alt kümesi: `+work-urgent/TODO`, `PRIORITY="A"`), tam metin arama.
- **Eylemler:** görünümden belgeye atlama, TODO değiştirme, yeniden zamanlama (sürükle bırak takvim), saat başlatma.
- **Capture:** TOML'da tanımlı şablonlarla hızlı not; hedef dosya ve başlık.
- **Refile:** klasör içi başlık seçici, bulanık arama.
- **Saat:** clock in ve out, `:LOGBOOK:` CLOCK satırları, günlük ve haftalık toplam, çalışan saat göstergesi.
- Tek dosya modu varsayılandır; agenda yalnızca çalışma klasörü açıldığında görünür.

---

## 14. Ayarlar ve yapılandırma

Katmanlar, sonraki öncekini geçersiz kılar:

1. Yerleşik varsayılanlar
2. Kullanıcı `settings.toml`
3. Çalışma klasörü `.kalem/settings.toml`
4. Belge `#+` anahtar kelimeleri (yalnızca belge davranışı için)

Örnek `settings.toml`:

```toml
[editor]
font_family = "Georgia"
font_size = 16
line_width = 80
keymap_profile = "word"        # "word" | "org"
show_source_markers = "cursor" # "cursor" | "always" | "never"

[org]
todo_keywords = ["TODO", "NEXT", "|", "DONE", "CANCELLED"]  # belge #+TODO yoksa
log_done = "time"
assets_dir = "{name}_assets"

[export]
pdf_engine = "auto"            # "auto" | "latexmk" | "tectonic"
pandoc_path = ""

[plugins]
enabled = ["com.example.wordcount"]
```

---

## 15. Performans hedefleri

| Ölçüt | Hedef |
|---|---|
| Soğuk açılış, boş belge | 300 ms altı |
| 1 MB belge açılış | 200 ms altı |
| 10 MB belge, etkileşime kadar | 1 s altı |
| Tuş → ekran gecikmesi | 16 ms altı, p99 33 ms altı |
| Artımlı parse, tipik düzenleme | 2 ms altı |
| Bellek, boş belge | 80 MB altı |
| Bellek, 10 MB belge | 500 MB altı |
| Binary boyutu, matematik fontları dahil | 40 MB altı |
| 10 MB belge kaydetme | 100 ms altı |

Strateji: rope, artımlı CST, sanallaştırılmış render, tembel model, arka plan işleri, formül ve resim önbelleği. Her hedef için benchmark CI'da çalışır; gerileme PR'ı engeller.

---

## 16. Test stratejisi

| Katman | Yöntem |
|---|---|
| org-syntax | Snapshot (insta), round-trip fuzz, Emacs diferansiyel, proptest ile rastgele belge üretimi, criterion benchmark |
| org-model | Birim testleri; kalıtım, tekrar kuralları, istatistik çerezleri için tablo tabanlı testler |
| org-edit | Her komut için önce ve sonra snapshot; geri alma özelliği testi (`undo(redo(x)) == x`) |
| org-table | Emacs'ta hesaplanmış tablo korpusu; formül parser snapshot'ları |
| org-math | Formül korpusu görüntü snapshot'ları (piksel farkı eşiği) |
| org-export | Her backend için snapshot; ox.el çıktısıyla karşılaştırma korpusu |
| org-babel | Sahte yürütücülerle birim testleri; gerçek yorumlayıcılarla entegrasyon (CI'da opsiyonel) |
| kalem-core | Komut kaydı, tuş haritası, when-clause birim testleri |
| kalem-script | API sözleşme testleri; d.ts ile gerçek bağlamaların tutarlılığı; zaman ve bellek limitleri |
| kalem-ui | gpui test harness ile widget testleri; manuel sürüm kontrol listesi |
| Uçtan uca | Korpus dosyalarını aç, betikle düzenle, kaydet, Emacs ile doğrula |

Korpus dosyalarının lisansı depo içinde belgelenir. Kullanıcı verisi asla korpusa alınmaz.

---

## 17. Paketleme ve dağıtım

- Tek binary; `cargo-dist` ile sürüm otomasyonu.
- macOS: `.app` paketi, imzalama ve notarizasyon, Homebrew cask.
- Windows: MSI ve taşınabilir zip; imzalama.
- Linux: AppImage, Flatpak, `.deb` ve `.rpm` ikincil; AUR topluluk.
- Otomatik güncelleme: 4. fazda; ilk sürümlerde bildirim ve bağlantı.
- Harici araçlar (pandoc, tectonic, yorumlayıcılar) paketlenmez; algılanır ve indirme yönlendirmesi yapılır.
- Sürümleme: semver; 1.0 öncesi 0.x, kırıcı değişiklikler CHANGELOG'da.

---

## 18. Açık kaynak ve topluluk

### 18.1 Lisans (D1)

İki makul seçenek:

| Seçenek | Artı | Eksi |
|---|---|---|
| MIT OR Apache-2.0 (Rust ekosistemi standardı) | En geniş yeniden kullanım; `org-*` crate'leri başka projelere kolay girer; katkı sürtünmesi az | Ticari kapalı çatal mümkün |
| GPL-3.0 | Emacs ve Org topluluğuyla lisans uyumu; kapalı çatala karşı koruma | Kütüphane olarak yeniden kullanımı daraltır; eklenti lisansı tartışmaları |

Öneri: `org-*` kütüphane crate'leri MIT OR Apache-2.0; uygulama da aynı (basitlik). Kullanıcı GPL istiyorsa yalnızca `kalem-*` crate'leri GPL olabilir.

### 18.2 Depo ve süreç

- Tek depo (monorepo), GitHub. CI: Linux, macOS, Windows; Emacs diferansiyel testleri Linux'ta.
- `CONTRIBUTING.md`, davranış kuralları, "iyi ilk konu" etiketleri, PR şablonu.
- Büyük kararlar için `rfcs/` klasörü; bu belge ilk RFC'dir.
- `CHANGELOG.md`, semver, düzenli sürüm ritmi.
- Belgeler: mdBook ile kullanıcı kılavuzu, eklenti API'si, mimari. Belgeler `.org` formatında yazılır ve uygulamanın kendi dışa aktarmasıyla üretilir (dogfooding).
- Dil: depo ve kod İngilizce; arayüz İngilizce ve Türkçe; bu belge yayın öncesi çevrilir.

### 18.3 Emacs topluluğuyla ilişki

- Org geliştirici listesine duyuru; format değişikliği önerilmez.
- Org Syntax belgesindeki belirsizlikler üst akıma rapor edilir.
- Diferansiyel test altyapısı Org'un kendisine de faydalı olabilir; paylaşılır.

---

## 19. Riskler ve azaltma

| Risk | Olasılık | Etki | Azaltma |
|---|---|---|---|
| Rust'ta WYSIWYG düzenleme motoru beklenenden zor | Yüksek | Yüksek | 3 haftalık gpui spike'ı, açık go/no-go; Tauri yedek yolu; çekirdek bağımsız |
| gpui API kırıcı değişiklikleri | Orta | Orta | Sürüm sabitleme; UI katmanını ince tutma; Zed sürümlerini izleme |
| Org sözdizimi belirsizlikleri | Yüksek | Orta | Emacs diferansiyel test; belirsizlikleri belgeleme; org-element davranışını esas alma |
| Kapsam şişmesi | Yüksek | Yüksek | Hedef olmayanlar listesi; faz çıkış ölçütleri; her özelliğe faz numarası |
| Tek geliştirici tükenmesi | Orta | Yüksek | Küçük yayınlanabilir parçalar (önce parser crate'i); erken topluluk; yol haritasında tampon |
| Matematik render kalitesi yetersiz | Orta | Orta | D4 için prototip ve korpus ölçümü; iki alternatif |
| Tablo hesaplarında Emacs'tan sapma | Orta | Orta | Emacs'ta hesaplanmış korpus; ondalık aritmetik; sapmaları belgeleme |
| Benimsenme düşük | Orta | Yüksek | "Org için Typora" konumlandırması; P1 ve P4 kişiliklerine erken erişim; Emacs topluluğuna duyuru |
| Harici araç bağımlılığı (pandoc, TeX) kullanıcıyı yorar | Orta | Düşük | Algılama ve yönlendirme; yerleşik HTML ve Markdown; tectonic isteğe bağlı indirme |
| Eklenti güvenlik açığı | Düşük | Yüksek | İzin modeli; sandbox; zaman ve bellek limitleri; güvenlik politikası |

---

## 20. Yol haritası

Süreler tek geliştirici için kaba tahmindir. Her fazın çıkış ölçütü karşılanmadan sonraki faza geçilmez.

### Faz 0: Keşif ve temel (2 ila 3 ay)

- Depo, CI, lisans, ad.
- orgize değerlendirmesi ve parser kararı (D2).
- `org-syntax`: tam kapsamlı kayıpsız parser, Emacs diferansiyel test altyapısı, korpus.
- `kalem-cli`: `parse`, `check`, `diff-emacs`.
- gpui spike ve UI kararı (D3).
- Matematik render prototipi ve kararı (D4).

**Çıkış:** korpusta yüzde yüz round-trip; Emacs diferansiyelde yüzde 99; spike raporu ve D2, D3, D4 kararları.

### Faz 1: MVP, "Org için Typora" (4 ila 6 ay)

- `org-model`, `org-edit` çekirdeği; geri al ve yinele.
- `kalem-core`: komut kaydı, tuş haritası, ayarlar.
- `kalem-ui`: WYSIWYG editör (başlıklar, katlama, paragraf, vurgular, listeler, onay kutuları, bağlantılar, temel tablo, kaynak bloğu), kaynak görünümü, taslak paneli, komut paleti, bul ve değiştir, durum çubuğu.
- TODO döngüsü, öncelik, etiketler, zaman damgası gösterimi.
- Dosya açma, kaydetme, dış değişiklik algılama.
- Tema ve yazı tipi ayarları; İngilizce ve Türkçe arayüz.
- İlk genel sürüm (0.1).

**Çıkış:** Org Manual kaynağı açılıp düzenlenip diff üretmeden kaydediliyor; performans hedeflerinin açılış ve tuş gecikmesi maddeleri tutuyor; on dış kullanıcıdan geri bildirim.

### Faz 2: Belge yazarı (4 ay)

- `org-table`: TBLFM motoru, formül çubuğu, sıralama, CSV.
- `org-math`: satır içi formül önizleme.
- `org-export`: HTML, LaTeX, Markdown, düz metin; PDF üretimi; pandoc köprüsü (DOCX, ODT, EPUB); içe aktarma.
- `org-cite` ve hayagriva.
- Resimler, dipnotlar, planlama satırları, özellik çekmeceleri, içindekiler.
- Zengin metin kopyalama ve yapıştırma.

**Çıkış:** bir kitap bölümü (şekil, tablo, formül, atıf) LaTeX ve PDF'e hatasız çıkıyor; tablo korpusu Emacs'la aynı hesaplıyor.

### Faz 3: Genişletilebilirlik ve görevler (4 ay)

- `kalem-script`: QuickJS, API, d.ts üretimi, eklenti yükleyici, izinler, konsol; `init.js`.
- Örnek eklentiler ve şablon depo.
- `org-babel`: shell, python, js, gnuplot; güven modeli; tangle.
- `org-agenda`: çalışma klasörü, ajanda görünümleri, capture, refile, saat.
- Yazım denetimi, reveal.js ve Beamer dışa aktarma.

**Çıkış:** üç topluluk eklentisi; agenda günlük kullanımda; Babel ile gnuplot grafiği belgede.

### Faz 4: Olgunlaşma

- Lua ikinci betik dili (D10), WASM eklentileri, dış süreç protokolü.
- `:session` ve `:noweb` Babel; sütun görünümü; org-habit.
- Sunum modu; gömülü PDF önizleme; otomatik güncelleme.
- Erişilebilirlik iyileştirmeleri; performans ayarı; 1.0.

---

## 21. Açık kararlar

| ID | Karar | Seçenekler | Öneri | Durum |
|---|---|---|---|---|
| D1 | Lisans | MIT OR Apache-2.0; GPL-3.0; karma | MIT OR Apache-2.0 her yerde | Açık |
| D2 | Parser temeli | orgize bağımlılığı; orgize fork; sıfırdan | Değerlendirme sonrası | Açık |
| D3 | UI çerçevesi | gpui; Tauri + ProseMirror; iced/floem | gpui, spike ile doğrulanacak | Açık |
| D4 | Matematik motoru | mitex + typst; ReX; KaTeX | mitex + typst prototipi | Açık |
| D5 | tectonic | Gömme; ayrı indirme; yalnızca sistem TeX | Ayrı indirme | Açık |
| D6 | API tanım kaynağı | Rust makroları; ayrı IDL; elle d.ts | Rust'ta tek tanım, d.ts ve Lua açıklamaları üretilir | Açık |
| D7 | Proje adı | – | Kalem | Karar verildi: **Kalem**. crates.io ve GitHub adı uygunluğu kontrol edilecek |
| D8 | Agenda indeks depolama | Bellek içi; SQLite; özel dosya | Bellek içi, disk önbelleği sonra | Açık |
| D9 | Yapılandırma formatları | TOML + JS; yalnızca JS; JSON | TOML + init.js + keymap.json | Açık |
| D10 | Lua desteği zamanı | Faz 3; Faz 4; hiç | Faz 4, talebe göre | Açık |
| D11 | Eklenti panellerinde webview | Asla; isteğe bağlı | Asla; JSON widget ağacı | Açık |
| D12 | Çoklu belge | Tek pencere tek belge; sekmeler; çoklu pencere | Sekmeler, Faz 2 | Açık |
| D13 | Zaman kütüphanesi | jiff; chrono | jiff | Açık |

---

## 22. Sözlük

| Terim | Anlam |
|---|---|
| CST | Concrete Syntax Tree; boşluk ve işaretler dahil her token'ı koruyan ağaç |
| AST | Abstract Syntax Tree; yalnızca anlamı koruyan ağaç |
| Rope | Büyük metinlerde hızlı ekleme ve silme sağlayan veri yapısı |
| Round-trip | Metni ayrıştırıp geri yazınca aynı metnin çıkması |
| Öğe (element) | Org'da satır düzeyinde yapı: başlık, paragraf, tablo, blok |
| Nesne (object) | Org'da satır içi yapı: vurgu, bağlantı, zaman damgası |
| TBLFM | Tablo formülü satırı `#+TBLFM:` |
| Babel | Org'un kod bloğu çalıştırma sistemi |
| Tangle | Kod bloklarını kaynak dosyalara yazma |
| Agenda | Zamanlanmış ve görev öğelerinin çoklu dosya görünümü |
| Capture | Şablonla hızlı not ekleme |
| Refile | Bir başlığı başka bir başlık altına taşıma |
| Drawer | `:AD:` ... `:END:` arasındaki katlanabilir bölüm |
| Planning | Başlık altındaki SCHEDULED, DEADLINE, CLOSED satırı |
| org-cite | Org'un yerleşik atıf sözdizimi |
| Backend | Dışa aktarma hedefi (HTML, LaTeX, ...) |
| When-clause | Bir komutun veya kısayolun geçerli olduğu bağlam ifadesi |
| Spike | Bir teknik riski ölçmek için yapılan zaman sınırlı prototip |

---

## 23. Kaynaklar

- Org Syntax: https://orgmode.org/worg/org-syntax.html
- Org Manual: https://orgmode.org/manual/
- org-element.el: Emacs kaynak ağacı, `lisp/org/org-element.el`
- orgize: https://github.com/PoiScript/orgize
- rowan: https://github.com/rust-analyzer/rowan
- gpui: https://github.com/zed-industries/zed/tree/main/crates/gpui
- rquickjs: https://github.com/DelSkayn/rquickjs
- quickjs-ng: https://github.com/quickjs-ng/quickjs
- typst: https://github.com/typst/typst
- mitex: https://github.com/mitex-rs/mitex
- hayagriva: https://github.com/typst/hayagriva
- tectonic: https://tectonic-typesetting.github.io/
- spellbook: https://github.com/helix-editor/spellbook
- extism: https://extism.org/
- pandoc: https://pandoc.org/
- Typora (ürün modeli): https://typora.io/
- Neovim Lua API (eklenti modeli referansı): https://neovim.io/doc/user/lua.html
- Figma eklenti sandbox'ı (QuickJS kullanımı): https://www.figma.com/plugin-docs/how-plugins-run/
