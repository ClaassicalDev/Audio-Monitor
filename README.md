# Audio Monitor — Studio WASAPI Per-Process Mixer & Sidechain DSP

Monitor de audio nativo, portable y de ultra baja latencia para **Windows 10/11** escrito en **Rust** (`egui` + `glow` + `WASAPI Process Loopback`).

Diseñado para reemplazar por completo el uso de **OBS Studio** como monitor de audio personal: captura cada aplicación o juego de forma independiente en memoria (`VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK`), aplica mezcla con faders de curva cúbica idéntica a OBS, mejora la claridad de las voces de Discord en tiempo real y ejecuta **Ducking Espectral Híbrido** + **Modo Enfoque (Prioridad 2)** exclusivamente en tus audífonos, sin alterar el volumen que escuchan tus espectadores en **Discord Screen Share** o streams.

---

## Características Principales

### 1. Captura por Proceso (`WASAPI Process Loopback`) + Entrada USB Hardware
- **Aislamiento para Discord Screen Share:** Las aplicaciones pueden rutearse a `VB-Audio Virtual Cable` y ser capturadas individualmente por `AudioMonitor.exe` mediante `ActivateAudioInterfaceAsync("VAD\\Process_Loopback")` con `PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE`.
- **Resolución inteligente de procesos y títulos (`src/audio/process.rs`):** Detecta procesos raíz (`CreateToolhelp32Snapshot`) y títulos de ventanas visibles (`EnumWindows` + `GetWindowTextW`) sin abrir handles `OpenProcess` hacia ningún juego (**0% riesgo con sistemas Anti-Cheat** como *ACE*, *HoYoKProtect*, *Vanguard* o *Ricochet*).
- **Soporte de entrada física (`Digitech`):** Monitoreo de baja latencia para interfaces/pedaleras USB (ej. **DigiTech RP500**), con detección automática de conexión física.

### 2. Claridad Vocal DSP para Discord (`VoiceClarityProcessor`)
Cadena DSP en tiempo real a `48,000 Hz` (`f32` estéreo) en `src/audio/dsp.rs`:
1. **High-Pass Filter (Butterworth 2º orden @ 95 Hz):** Elimina golpes de escritorio, retumbo y efecto proximidad.
2. **Compuerta Anti-Siseo / Expansor Descendente (`-46 dB`, piso `-14 dB`):** Atenúa suavemente el ruido de ventiladores y estática de micrófonos durante las pausas.
3. **Filtro Anti-Encajonamiento (`Peaking EQ @ 240 Hz`, `-3.0 dB`):** Limpia la resonancia nasal típica de diademas gaming.
4. **Realce de Presencia Vocal (`Peaking EQ @ 3.2 kHz`, `+4.0 dB`):** Hace que las voces corten con nitidez sobre cualquier efecto de juego.
5. **De-Esser Dinámico Dividido (`Band-Split @ 6.2 kHz`):** Comprime selectivamente las sibilancias (`S`, `CH`, `T`) sin apagar el resto de la voz.
6. **Aire / Brillo (`High-Shelf @ 6.5 kHz`, `+1.8 dB`):** Recupera definición en voces comprimidas por Opus.
7. **Nivelador Dinámico Vocal + Soft-Knee Limiter (`-0.5 dB`):** Empareja amigos que hablan muy bajo y frena gritos repentinos.
- **3 Perfiles en 1 clic:** `Nítido / Presencia`, `Extra Claridad` y `Broadcast (Cálido)`.

### 3. Compresor Sidechain con Ducking Espectral Híbrido (Estilo Trackspacer)
- **Ducking Espectral Híbrido (`300 Hz – 4 kHz`):**
  - Cuando alguien habla en Discord (`SIDECHAIN`), la banda media vocal del juego recibe el **125% de la reducción** (abriendo un espacio limpio para la voz), mientras que los subgraves (`< 300 Hz`) y agudos (`> 4 kHz`) bajan un **75%** (ajustable de `40%` a `100%`).
- **Detector Sidechain Vocal (`200 Hz – 3.8 kHz`):** Ignora golpes de mesa o clics graves para no bajar el juego por accidente.
- **Anti-Bombeo (*Hold* de `85 ms`) y Piso de Atenuación (`-15 dB`):** Evita que el volumen suba y baje entre sílabas y evita que el juego quede en silencio total.
- **4 Presets integrados:** `Gaming Pro`, `Competitivo`, `Suave / Música` y `OBS Clásico`.

### 4. Modo Enfoque / Jerarquía de 3 Niveles (`Prioridad 2`)
- **Nivel 1 (`SIDECHAIN` — Discord):** Prioridad absoluta; atenúa tanto a canales `PRIO 2` como a canales normales.
- **Nivel 2 (`PRIO 2` — Navegadores, Spotify o juego secundario):** Cuando una app marcada con `PRIO 2` emite audio, atenúa suavemente a los canales normales (`-12 dB` por defecto, con retención de `350 ms`).
- **Nivel 3 (Canales Normales con `COMPRESOR`):** Reciben la atenuación combinada de Nivel 1 + Nivel 2.

### 5. Doble Interfaz: Consola de Estudio (`1140×740`) + Flyout Estilo EarTrumpet (`540×600`)
- **Arranque silencioso en System Tray:** Al ejecutarse, inicia directamente minimizado en la bandeja del sistema (junto al reloj de Windows) tras inicializar el swapchain OpenGL en el primer frame.
- **Clic izquierdo en el icono de bandeja (Modo Flyout EarTrumpet):** Despliega el mezclador rápido compacto (`540×600`) anclado sobre el reloj (`SPI_GETWORKAREA`), con auto-ocultado al hacer clic fuera y botón de fijado (`Fijado` / `Auto-ocultar`).
- **Clic derecho → *Abrir Ventana Normal* (o doble clic / botón `[Ventana Normal]`):** Abre la consola completa (`1140×740`) centrada en pantalla, con alineación matemática de columnas (`0.0 px` de desviación entre filas) y cabecera de 3 filas sin solapamientos.
- **Arquitectura Win32 100% Lock-Free (`src/tray.rs`):** `tray_wnd_proc` jamás invoca funciones síncronas (`SetWindowPos` / `ShowWindow`) hacia otro hilo; escribe únicamente en un `AtomicU8` (`GLOBAL_PENDING_ACTION`) y despierta la ventana con `ShowWindowAsync` + `PostMessageW`, ejecutando `SetWindowPos` siempre desde el hilo dueño (`winit`) para evitar cualquier bloqueo con `explorer.exe`.

---

## Estructura del Proyecto

```text
Audio-Monitor/
├── Cargo.toml                 # Dependencias (eframe/egui 0.37, wasapi 0.27, windows 0.62)
├── config.json                # Estado persistente de los 17 canales, volúmenes y DSP
├── AudioMonitor.exe           # Binario portable compilado (x86_64-pc-windows-gnu)
├── README.md                  # Documentación general del repositorio
├── GEMINI.md                  # Contexto detallado y guía para Antigravity / Gemini
├── AGENTS.md                  # Guía de arquitectura e invariantes para agentes de IA (Codex/Cursor)
├── CLAUDE.md                  # Guía de arquitectura e invariantes para Claude Code
└── src/
    ├── main.rs                # Punto de entrada, carga de fuentes nativas Windows y opciones de viewport
    ├── config.rs              # Estructuras serializables (AppConfig, ChannelConfig, CompressorConfig) y presets
    ├── tray.rs                # Icono nativo Win32 Shell_NotifyIconW, menú contextual y worker asíncrono
    ├── audio/
    │   ├── mod.rs
    │   ├── capture.rs         # Workers WASAPI Process Loopback y captura de hardware USB (DigiTech)
    │   ├── dsp.rs             # Biquad IIR, VoiceClarityProcessor, SpectralDuckingFilter, Sidechain y FocusDucker
    │   ├── process.rs         # Snapshot de procesos (CreateToolhelp32Snapshot) y títulos de ventana (EnumWindows)
    │   ├── renderer.rs        # Mezclador maestro a 48 kHz float32, ruteo de salida y cálculo de vúmetros
    │   └── ring_buffer.rs     # Buffer circular lock-free entre hilos de captura y el mezclador
    └── ui/
        ├── mod.rs
        ├── app.rs             # Interfaz egui (Modo Estudio 1140x740 + Modo Flyout 540x600 con geometría fija)
        └── widgets.rs         # Vúmetros segmentados LED, botones vectoriales de Mute y píldoras de canal
```

---

## Compilación

### Compilación cruzada desde Linux (`x86_64-pc-windows-gnu` con `llvm-mingw`)
```bash
export PATH="$HOME/.local/llvm-mingw/bin:$HOME/.cargo/bin:$PATH"
cargo build --release --target x86_64-pc-windows-gnu
```
El ejecutable portable se genera en `target/x86_64-pc-windows-gnu/release/AudioMonitor.exe`.

### Compilación nativa en Windows (MSVC o GNU)
```powershell
cargo build --release
```

---

## Continuidad para Agentes de IA
Si eres un agente de IA (**Antigravity**, **Claude Code**, **Codex**, **Cursor**, etc.) trabajando en este repositorio, lee **obligatoriamente** [`AGENTS.md`](./AGENTS.md) / [`GEMINI.md`](./GEMINI.md) antes de modificar cualquier archivo para respetar los invariantes de seguridad Anti-Cheat, el manejo asíncrono del System Tray (`explorer.exe`) y el flujo de compilación.
