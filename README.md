# imlec

Yazarken imlecin peşinden giden partikül efektleri — Notepad'den nvim'e, kitty'den
tarayıcıya kadar **her uygulamada**. VS Code'daki Power Mode eklentisinin işletim
sistemi seviyesine taşınmış hâli.

- **Arch Linux / Hyprland** ve diğer wlroots tabanlı Wayland compositor'ları (Sway, river, niri, Wayfire)
- **X11** (picom gibi bir compositing manager ile)
- **Windows 10/11**
- Aynı `config.toml`, her iki sistemde de aynı deneyim

---

## Kurulum

### Linux (Arch / Hyprland)

```bash
curl -fsSL https://raw.githubusercontent.com/koinkafasi/imlec/main/install.sh | bash
```

Kurulum betiği binary'yi `~/.local/bin/imlec` altına koyar, `input` grubuna
eklenmen için izin ister ve istersen systemd user servisini etkinleştirir.

`input` grubu şart: Wayland güvenlik nedeniyle global klavye olaylarını
uygulamalara vermiyor, bu yüzden imlec doğrudan `/dev/input` üzerinden okuyor.
Bu sayede compositor'dan bağımsız, her uygulamada çalışıyor.

```bash
sudo usermod -aG input "$USER"   # betik sormazsa
```

Değişikliğin geçerli olması için oturumu kapatıp açman gerekir.

Hyprland'de systemd yerine doğrudan autostart isteyen:

```
exec-once = ~/.local/bin/imlec
```

### Windows

```powershell
irm https://raw.githubusercontent.com/koinkafasi/imlec/main/install.ps1 | iex
```

`%LOCALAPPDATA%\imlec` altına kurar, Startup klasörüne kısayol koyar ve başlatır.
Sistem tepsisindeki simgeye sağ tıklayarak efektleri açıp kapatabilir, config
dosyasını açabilir veya çıkabilirsin.

### Kaynaktan

```bash
cargo build --release --bin imlec
```

Linux tarafında hiçbir sistem kütüphanesi gerekmiyor — Wayland, X11 ve
rasterizer bağımlılıklarının hepsi saf Rust.

---

## Ayarlar

### Canlı ayar arayüzü

```bash
imlec tune
```

Terminal içinde açılan editörde ok tuşlarıyla renk, şekil, boyut, hız, yerçekimi
ve ömrü değiştirirsin; her değişiklik anında dosyaya yazılır ve çalışan overlay
yarım saniye içinde uygular. Yani ekranda gerçek efekti görerek ayar yaparsın.

| tuş | iş |
|---|---|
| `←` `→` | değeri azalt / artır |
| `Shift` + ok | 10 kat büyük adım |
| `↑` `↓` | alan seç |
| `Tab` | General / Typing / Deleting arası geç |
| `[` `]` | palet rengi seç |
| `a` / `x` | palete renk ekle / çıkar |
| `r` | varsayılanlara dön |
| `q` | çık |

Renkler HSV olarak ayarlanır (hue / saturation / value), hex yazmaya gerek yok.
Dosyadaki açıklama satırları düzenleme sonrası korunur.

Windows'ta tepsi simgesine sağ tıkla > **Tune settings (live)**.

### Config dosyası

```bash
imlec --print-config-path
```

- Linux: `~/.config/particle-cursor/config.toml`
- Windows: `%APPDATA%\particle-cursor\config.toml`

Dosya kaydedildiği anda ayarlar canlı yüklenir, yeniden başlatmaya gerek yok.

```toml
[general]
enabled = true
fps = 60
max_particles = 600

# Partikül boyu = cursor_height_px * size_ratio.
# 20 * 0.5 = 10px, yani imleç boyunun yarısı.
cursor_height_px = 20.0

# Hızlı ardışık tuşlar partikül sayısını artırır (Power Mode combo).
combo_enabled = true
combo_max_multiplier = 2.5

[typing]
count = 6
shape = "circle"      # circle | square | triangle | diamond | star | spark | hexagon
size_ratio = 0.5
lifetime_ms = 500
speed = 130.0
direction_deg = -90.0 # 0 sağ, -90 yukarı, 90 aşağı
spread_deg = 140.0    # 360 = her yöne
gravity = 320.0       # negatif değer yukarı süzülür
shrink = true

[typing.color]
mode = "palette"
colors = ["#ff2d95", "#ff9f1c", "#2de2e6", "#a06cff", "#f9f871"]

[deleting]
count = 8
shape = "spark"
direction_deg = 90.0
spread_deg = 360.0
gravity = -60.0

[deleting.color]
mode = "fixed"
color = "#ff3b30"
```

### Renk modları

| mode | alanlar |
|------|---------|
| `fixed` | `color = "#ff2d95"` |
| `palette` | `colors = ["#ff2d95", "#2de2e6"]` |
| `gradient` | `from`, `to` |
| `rainbow` | `speed`, `saturation`, `value` |

Yazma ve silme için ayrı şekil, renk, boyut ve fizik ayarlanabilir.

---

## Windows imzalama

Yayınlanan binary'ler şu an **kod imzalı değil**. Windows 11'de **Smart App
Control** açıksa imzasız uygulamayı çalıştırmaz ve şunu görürsün:

```
Uygulama Denetimi ilkesi bu dosyayı engelledi.
```

Durumunu kontrol et:

```powershell
(Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\CI\Policy' -Name VerifiedAndReputablePolicyState).VerifiedAndReputablePolicyState
```

`0` kapalı, `1` zorlama (engeller), `2` değerlendirme modu.

### Kalıcı çözüm: kod imzalama sertifikası

Sertifika satın alınması gereken bir şey; kod tarafında halledilemez. CI zaten
imzalamaya hazır — aşağıdaki secret'ları depoya eklediğin an her release
otomatik imzalanır (`.github/workflows/release.yml`):

| secret | ne |
|---|---|
| `AZURE_TENANT_ID` | Azure dizin kimliği |
| `AZURE_CLIENT_ID` | uygulama kaydı kimliği |
| `AZURE_CLIENT_SECRET` | uygulama gizli anahtarı |
| `AZURE_SIGNING_ENDPOINT` | ör. `https://weu.codesigning.azure.net` |
| `AZURE_SIGNING_ACCOUNT` | Trusted Signing hesap adı |
| `AZURE_CERT_PROFILE` | sertifika profili adı |

Seçenekler:

| yol | maliyet | CI'da çalışır | SAC'i hemen aşar |
|---|---|---|---|
| **Azure Trusted Signing** | ~10 USD/ay | evet | genelde evet |
| EV kod imzalama sertifikası | ~300-600 USD/yıl | donanım token gerekir, zor | evet |
| OV kod imzalama sertifikası | ~200-400 USD/yıl | HSM ile | hayır, itibar birikmesi gerekir |
| Kendinden imzalı sertifika | ücretsiz | evet | **hayır** |

Azure Trusted Signing bireysel hesaplara da açık ama kimlik doğrulaması ister
(bireyler için 3+ yıllık doğrulanabilir geçmiş şartı var). Kendinden imzalı
sertifika bu sorunu **çözmez** — Windows onu tanımaz.

### Geçici çözüm

Smart App Control'ü kapatmak:
Ayarlar > Gizlilik ve güvenlik > Windows Güvenliği > Uygulama ve tarayıcı
denetimi > Smart App Control ayarları.

> **Bunu geri alamazsın.** Smart App Control kapatıldıktan sonra yeniden
> açılamaz; açmak Windows'u sıfırdan kurmayı gerektirir. Sadece ne yaptığını
> bilerek yap.

İmza gelene kadar indirdiğin dosyayı `SHA256SUMS` ile doğrulayabilirsin:

```powershell
Get-FileHash imlec-setup-x64.exe -Algorithm SHA256
```

---

## Güncelleme

```bash
imlec update
```

GitHub Releases'ten platformuna uygun son sürümü indirir ve çalışan binary'yi
değiştirir. Sadece kontrol etmek için `imlec update --check`.

Overlay çalışırken günde bir kez arka planda yeni sürüm kontrolü yapılır ve
**sadece log'a yazar** — sessizce kurulum yapmaz. Kurulum her zaman senin
`imlec update` demenle olur.

`/usr/bin`, `/opt` gibi paket yöneticisine ait konumlara kurulmuşsa kendini
güncellemeyi reddeder; o durumda `pacman`/AUR ile güncellenir. Windows'ta
installer ile kurduysan yeni `imlec-setup-x64.exe` de üstüne kurar.

---

## Performans

Her karede tüm ekran değil, sadece partiküllerin kapladığı dikdörtgen yeniden
çiziliyor. Boşta hiç partikül yokken render döngüsü tamamen duruyor ve CPU
kullanımı sıfıra iniyor.

- **Wayland**: her monitör için ayrı layer surface, kısmi damage ile çift tamponlama
- **X11**: 32-bit ARGB override-redirect pencere, sadece kirli bölge `PutImage`
- **Windows**: katmanlı pencere her karede partikül sınırına göre küçültülüp taşınıyor,
  böylece `UpdateLayeredWindow` masaüstü boyutundan bağımsız küçük bir bitmap yüklüyor

`max_particles` ve `fps` ile üst sınırı istediğin gibi kısabilirsin.

---

## Nasıl çalışıyor

| | Linux (Wayland) | Linux (X11) | Windows |
|---|---|---|---|
| Tuş yakalama | `evdev` (`/dev/input`) | `evdev` | `WH_KEYBOARD_LL` hook |
| İmleç konumu | Hyprland IPC `cursorpos` | `QueryPointer` | `GetCursorPos` |
| Overlay | `wlr-layer-shell`, boş input region | override-redirect + XShape | `WS_EX_LAYERED \| WS_EX_TRANSPARENT` |
| Çizim | `tiny-skia` → `wl_shm` | `tiny-skia` → `PutImage` | `tiny-skia` → DIB |

Overlay tıklamaları geçirir; altındaki uygulama efektin varlığından habersizdir.
Bu yüzden nvim, LazyVim, kitty, alacritty, foot, wezterm, Notepad, tarayıcı —
hepsinde ayrı entegrasyon olmadan çalışır.

---

## Bilinen sınırlar

- **Gerçek metin imleci değil, fare imleci takip edilir.** Wayland'de bir
  uygulamanın caret konumunu okumanın taşınabilir bir yolu yok.
- **GNOME (Mutter) desteklemiyor** — `wlr-layer-shell` protokolünü uygulamıyor.
- **Hyprland dışındaki Wayland compositor'larında** imleç konumu göreli fare
  hareketinden tahmin edilir ve gerçek konumdan sapabilir. Hyprland ve X11'de konum tamdır.
- **Tam ekran exclusive uygulamalar** (bazı oyunlar) overlay'i örtebilir.
  Borderless windowed modda sorun yoktur.
- **Saf TTY**'de çalışmaz, bir compositor gerekir.
- macOS backend'i henüz yok; mimari eklemeye uygun.

---

## Lisans

MIT
