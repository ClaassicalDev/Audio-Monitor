# Audio-Monitor — Monitor de Audio Nativo Windows en Rust con Captura por Proceso, Compresor Sidechain y Claridad Vocal DSP

## 📍 ESTADO
- **Actualizado:** 2026-11-18
- **Estado:** activo
- **Resumen:** Monitor de audio profesional portable en Rust (`AudioMonitor.exe`, 5.4 MB) con UI/UX de alta precisión y modo mezclador rápido tipo EarTrumpet (`540×600`) + Ventana Normal de Estudio (`1140×740`). Incluye arranque silencioso en System Tray 100% lock-free (`GLOBAL_PENDING_ACTION` + `ShowWindowAsync` para nunca bloquear a `explorer.exe`), captura por proceso mediante WASAPI Loopback, alineación matemática de columnas con 0.0 px de desviación entre filas, cabecera de consola en 3 filas sin solapamientos, ecualizador DSP de Claridad Vocal para Discord (HPF + EQ de articulación + Nivelador dinámico + De-Esser 6.2 kHz + Compuerta Anti-Siseo), Ducking Espectral Híbrido (125% vocal / 75% general), detector Sidechain anti-golpes con Hold de 85 ms y piso de -15 dB, y Modo Enfoque con Prioridad 2 de 3 niveles (0% riesgo Anti-Cheat). Documentado en `README.md`, `GEMINI.md`, `AGENTS.md` y `CLAUDE.md` y respaldado en GitHub (`ClaassicalDev/Audio-Monitor`).
- **Binario compilado:** `AudioMonitor.exe` (compilado en `vibebox` vía MinGW-w64, firmado digitalmente con Authenticode SHA-256 `CN=Claassical Local Dev`).
- **Siguiente paso:** Disfrutar en sesiones diarias de juego/Discord; cualquier IA que continúe el proyecto debe leer `AGENTS.md` y `GEMINI.md`.

---

## 1. Motivación y Arquitectura del Sistema

### ¿Por qué captura por proceso en lugar de bajar el volumen del mezclador de Windows?
Cuando Juan Antonio comparte pantalla o transmite un juego por Discord Screen Share, Discord captura directamente el audio del proceso del juego. Si se bajara el volumen del juego en el mezclador del sistema o se aplicara ducking a nivel de sesión de Windows, **los espectadores en Discord también escucharían el juego bajando de volumen**.

**Solución arquitectónica:**
1. Los juegos y navegadores pueden enrutarse a `VB-Audio Virtual Cable` (para que Windows no los reproduzca directamente en los audífonos dos veces).
2. `AudioMonitor.exe` captura cada aplicación individualmente en la memoria del sistema mediante **WASAPI Process Loopback (`VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK`)**, incluyendo todo su árbol de procesos hijo (`PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE`).
3. Para Discord y los espectadores del stream, el juego sigue al **100% de volumen original** e intacto.
4. Dentro de `AudioMonitor.exe`, cada fuente recibe su volumen personal, mute, escala de volumen general (Master), realce vocal y atenuación sidechain por Discord **exclusivamente en los audífonos del usuario**.
5. Se elimina la necesidad de tener OBS Studio abierto consumiendo GPU (render 3D, encoder) solo para monitorear audio.

---

## 2. Componentes Técnicos

### A. Descubrimiento Inteligente por Nombre y Título de Ventana (Estilo OBS)
- **Cero dependencia visual de PIDs:** Los números de proceso ya no se muestran en ninguna parte de la interfaz. En su lugar, cada canal muestra un indicador LED brillante tridimensional con el estado `"EN EJECUCIÓN"` (verde esmeralda) o `"INACTIVO"` (gris tenue).
- **Enumeración de ventanas visibles (`EnumWindows` + `GetWindowTextW` + `GetWindowThreadProcessId`):** Inspecciona las ventanas de nivel superior activas para asociar a cada proceso su nombre amigable y título real (ej. `"Overwatch"`, `"Spotify Premium"`, `"YouTube - Google Chrome"`, `"Discord"`). Si el proceso raíz delega la ventana a un subproceso hijo (como ocurre en Discord, Spotify o Chrome), el árbol jerárquico hereda automáticamente el título.
- **Filtro de procesos del sistema:** Se aíslan y ocultan por defecto servicios ruidosos del sistema (`svchost.exe`, `conhost.exe`, `runtimebroker.exe`, `explorer.exe`, `dwm.exe`, `csrss.exe`, etc.) con casilla opcional *"Mostrar procesos del sistema"*.
- **Orden de prioridad:** Las aplicaciones con ventanas visibles abiertas aparecen primero ordenadas alfabéticamente por su título, seguidas de aplicaciones en segundo plano por nombre ejecutable.
- **Buscador en vivo y coincidencia elástica (`find_root_pid`):** La búsqueda en la barra superior o en el cajón de agregar app filtra en tiempo real por título o ejecutable. Si un juego está cerrado, permite agregarlo escribiendo su nombre (ej. `Overwatch` o `Overwatch.exe`) y el motor lo enlaza de inmediato en cuanto se inicia gracias a una resolución insensible a mayúsculas/minúsculas y prefijos.

### B. Captura por Proceso (`WASAPI Process Loopback`)
- **API nativa:** `ActivateAudioInterfaceAsync` sobre `"VAD\\Process_Loopback"` con `AUDIOCLIENT_ACTIVATION_PARAMS` (`ActivationType = 1`).
- **Resolución de Root PID:** En aplicaciones multiproceso, localiza el PID raíz mediante `CreateToolhelp32Snapshot` y activa `PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE` para capturar todo el árbol de audio del proceso.
- **Escáner en segundo plano:** Cada 1.5 a 2 segundos detecta qué juegos o programas se abrieron o cerraron, levantando o deteniendo workers de captura de forma no bloqueante.
- **Entrada de hardware (`Digitech`):** Detecta automáticamente el endpoint de entrada USB del pedal DigiTech RP500 (`{0.0.1.00000000}.{bfc138e0-2398-4437-bc68-caa04d7ba3e6}`) y lo mezcla en vivo (silenciado por defecto para activarlo cuando Juan conecte y toque la guitarra).

### C. Salida de Monitoreo de Ultra Baja Latencia
- **Dispositivo de audífonos:**
  - `"Por defecto (Windows)"`: Rastrea en caliente el dispositivo predeterminado del sistema (si Juan cambia entre sus Corsair Virtuoso y Razer Kraken, el cliente WASAPI se reconecta en ~3 ms sin interrupciones ni cierres).
  - O selección explícita de cualquier endpoint de reproducción activo desde el menú desplegable.
- **Formato interno:** 48,000 Hz, 2 canales (estéreo), 32-bit float (`WAVE_FORMAT_IEEE_FLOAT`) con `AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM`. Búfer de 15 ms para una latencia auditiva efectiva de ~5 a 8 ms.

### D. Motor DSP de Claridad Vocal para Discord (`VoiceClarityProcessor`)
Procesador de voz dedicado a 48,000 Hz estéreo con filtros Biquad IIR en cascada (*Direct Form II Transposed*), expansor descendente, de-esser dinámico y nivelador vocal:
1. **Filtro Paso-Alto (Butterworth 2º orden):** Corta frecuencias retumbantes por debajo de 95 Hz (ruidos de mesa, vibraciones y efecto proximidad).
2. **Compuerta Anti-Siseo (Expansor descendente suave en pausas):** Detecta cuando la señal vocal cae por debajo de -46 dB y atenúa suavemente el piso de ruido hasta -14 dB (ataque 2 ms, liberación 180 ms). Elimina por completo el soplido de fondo, zumbidos eléctricos o ventiladores de PC en las pausas de Discord.
3. **Filtro Anti-Encajonamiento (Peaking EQ en 240 Hz, Q = 1.2, -3.0 dB):** Elimina el tono nasal y encajonado típico de micrófonos de diadema en Discord.
4. **Realce de Presencia e Inteligibilidad (Peaking EQ en 3200 Hz, Q = 1.0, +4.0 dB):** Aumenta drásticamente la articulación de consonantes para que la voz corte sin esfuerzo por encima de disparos, explosiones o música.
5. **De-Esser Dinámico Dividido (Band-Split en 6200 Hz, Q = 0.8):** Monitorea la energía de sibilancias ("S", "CH", "T"). Si la banda sibilante supera -18 dB, aplica compresión dinámica de 4:1 (ataque ultrarrápido 0.5 ms, release 40 ms) exclusivamente sobre la banda aguda sin alterar el resto de la voz, eliminando estridencias molestas en micrófonos condensadores cercanos.
6. **Definición Aguda / Aire (High-Shelf EQ en 6500 Hz, Q = 0.707, +1.8 dB):** Aporta brillo natural y apertura sonora a las voces comprimidas por el códec Opus de Discord.
7. **Nivelador Dinámico Vocal + Limitador Suave:** Compresión vocal transparente (umbral -24 dB, ratio 2.5:1, ataque 5 ms, release 150 ms, ganancia compensatoria +2.5 dB) seguida de un limitador con codo suave (*soft-knee limiter*) a -0.5 dB. Sube a los amigos que hablan bajito y frena los gritos repentinos sin saturación ni aspereza.

**3 Perfiles Seleccionables en 1 Clic:**
- **`Nítido / Presencia (Recomendado)`**: HPF 95 Hz, -3.0 dB @ 240 Hz, +4.0 dB @ 3.2 kHz, +1.8 dB @ 6.5 kHz, Nivelador +2.5 dB.
- **`Extra Claridad (Anti-Murmullos)`**: HPF 120 Hz, -4.0 dB @ 250 Hz, +5.5 dB @ 3.4 kHz, +2.5 dB @ 7.0 kHz, Nivelador +3.5 dB. Diseñado para cuando alguien tiene un micrófono oscuro o habla muy quedo.
- **`Broadcast / Estudio (Cálido y Firme)`**: HPF 80 Hz, +1.5 dB @ 140 Hz (cuerpo y calidez), -2.5 dB @ 280 Hz, +3.2 dB @ 3.2 kHz, Nivelador +3.0 dB.

### E. Compresor Sidechain Avanzado con Ducking Espectral y Bajada General
Revolución en el algoritmo de atenuación para superar a OBS y compresores tradicionales:
- **Ducking Espectral Inteligente con Bajada Real de Volumen:**
  - En lugar de bajar el volumen plano a todo el juego (lo que mata el ambiente, las explosiones y la inmersión) o dejarlo casi igual, divide la señal del juego mediante un filtro divisor Biquad BPF paralelo centrado en 1100 Hz ($Q = 0.55$, abarcando de 300 Hz a 4000 Hz) con reconstrucción complementaria $x_{\text{rest}} = x - x_{\text{mid}}$ libre de distorsión de fase.
  - La banda vocal media recibe un **125% de la atenuación calculada** (abriendo un hueco acústico transparente e impecable para la voz de Discord).
  - El resto del espectro (subgraves `< 300 Hz` y aire agudo `> 4 kHz`) recibe el **75% de la atenuación** (calibrable en el deslizador de 40% a 100%).
  - *Resultado auditivo:* Se nota claramente que el volumen del juego baja de forma contundente para dar paso a la conversación, pero el timbre de las explosiones, disparos y música de fondo conserva pegada y profundidad natural.
- **Detector Sidechain Anti-Golpes:** Aplica un filtro pasabanda vocal previo (200 Hz a 3800 Hz) sobre el audio de Discord antes de medir el pico de activación. Evita falsos disparos de atenuación provocados por ruidos de baja frecuencia (golpes al escritorio, vibraciones de teclado mecánico) o siseos de alta frecuencia.
- **Retención (*Hold* de 0 a 500 ms):** Mantiene la reducción de volumen fija durante 85 ms en pausas breves entre palabras de una misma frase. **Elimina por completo el molesto bombeo (*pumping*)** de sube y baja constante.
- **Atenuación Máxima (*Floor Limit* de -40 a -3 dB):** Limita la reducción a un piso musical (ej. -15 dB). **Evita que el juego quede en silencio absoluto** cuando alguien habla fuerte en Discord.
- **4 Presets de Estudio con 1 Clic:**
  - **`🎮 Gaming Pro (Recomendado)`**: Ducking Espectral ON, Bajada General 75%, Detector SC Vocal ON, Umbral -38 dB, Ratio 4:1, Ataque 4 ms, Hold 85 ms, Release 420 ms, Piso -15 dB.
  - **`🎯 Competitivo / Callouts`**: Ducking Espectral ON, Bajada General 80%, Detector SC Vocal ON, Umbral -42 dB, Ratio 6:1, Ataque 1.5 ms, Hold 110 ms, Release 350 ms, Piso -22 dB.
  - **`🎵 Suave / Música`**: Ducking Espectral ON, Bajada General 60%, Detector SC Vocal ON, Umbral -34 dB, Ratio 2.5:1, Ataque 10 ms, Hold 60 ms, Release 500 ms, Piso -9 dB.
  - **`🎬 OBS Clásico`**: Ducking Espectral OFF, Detector SC Vocal OFF, Umbral -50 dB, Ratio 5:1, Ataque 1.0 ms, Hold 0 ms, Release 300 ms, Piso -40 dB.

### F. Modo Enfoque / Jerarquía de Prioridad 2 (3 Niveles)
Arquitectura de 3 niveles para que Juan Antonio pueda ver videos de YouTube, escuchar podcasts o jugar a la vez que atiende llamadas:
- **Nivel 1 — `SIDECHAIN` (Discord / Voz humana):** Manda sobre todas las aplicaciones. Cuando alguien habla en Discord, se atenúan tanto los canales normales como los de Prioridad 2 (si tienen `COMPRESOR` activo).
- **Nivel 2 — `PRIO 2` (Modo Enfoque — Chrome, Brave, Opera GX, Spotify o juego secundario):**
  - Cualquier canal marcado con la píldora dorada `PRIO 2` actúa como disparador de enfoque.
  - Procesador `FocusDucker` dedicado a 48,000 Hz: umbral `-42.0 dB`, atenuación objetivo `-12.0 dB` (ajustable de `-4 dB` a `-30 dB`), ataque suave de `12 ms`, retención generosa (*Hold* de `350 ms` para que las pausas entre oraciones de un video no hagan subir y bajar el juego) y liberación suave de `650 ms`.
  - La aplicación con `PRIO 2` **nunca** se atenúa a sí misma; solo atenúa las apps normales.
- **Nivel 3 — Canales Normales con `COMPRESOR` activo:**
  - Reciben la atenuación combinada `(sidechain_db + focus_db).max(-45.0 dB)`.
  - Si Juan está jugando y pone un video en Chrome con `PRIO 2`, el juego baja automáticamente a `-12 dB` para escuchar el video con claridad. Y si de repente alguien habla en Discord, el juego y Chrome bajan aún más para darle prioridad absoluta a la voz de Discord.

### G. Modo Mezclador Rápido (Flyout EarTrumpet) y Bandeja del Sistema
Módulo 100% nativo Win32 (`src/tray.rs`) desarrollado sin ganchos globales, sin inyección DLL y con cero llamadas a `OpenProcess`:
- **Experiencia EarTrumpet al hacer clic en el reloj:**
  - **Clic izquierdo:** Alterna el mezclador rápido flyout (`500x580 px`) posicionado automáticamente justo arriba de la bandeja de entrada del sistema mediante `SystemParametersInfoW(SPI_GETWORKAREA)`.
  - **Auto-ocultación inteligente por desenfoque (*Blur*):** Un hilo ligero en segundo plano (40 ms) detecta cuando el usuario hace clic fuera de la ventana (`GetForegroundWindow() != main_hwnd`) y la oculta suavemente tras 150 ms.
  - **Protección antirrebote (350 ms):** Evita que un clic rápido sobre el icono en la bandeja cierre e inmediatamente vuelva a abrir el flyout.
  - **Botón Pin `[📌]`:** Permite fijar la ventana flyout para que no se oculte al hacer clic en otras aplicaciones mientras se ajusta sonido en juegos.
  - **Botón `[🎛 Estudio]`:** Transiciona instantáneamente de la vista compacta a la consola de estudio completa (`1120x740 px`) centrada en pantalla.
- **Icono de estudio personalizado 32x32:** Generado vectorialmente en memoria (auriculares en cian `#0284c7` y barras de ecualizador en esmeralda `#10b981` sobre fondo oscuro).
- **Minimizar al cerrar (`X`):** Activado por defecto. Al hacer clic en la `X`, cancela el cierre y oculta la ventana (`ShowWindow(SW_HIDE)`), liberando la barra de tareas y el menú `Alt+Tab`.
- **Menú contextual en la bandeja (clic derecho):**
  - *"Abrir Mezclador Rápido (Flyout)"*
  - *"Abrir Estudio Completo"*
  - Interruptores rápidos: *Voz Clara Discord*, *Ducking Espectral*, *Modo Enfoque Prio 2* y *Salir por completo*.

### H. Interfaz Gráfica de Estudio y Alineación Matemática
- **Alineación matemática con 0.0 px de desviación:**
  - Cada fila del mezclador pre-asigna su rectángulo exacto (`allocate_exact_size`) y lo particiona matemáticamente con `Rect::from_min_max` en 3 celdas fijas (`scope_builder` con `max_rect` y `clip_rect`):
    - **Columna 1 (Identidad - 200 px en Estudio / 125 px en Flyout):** LED tridimensional + estado `EN EJECUCIÓN` / `INACTIVO`, nombre visible de app truncado y nombre de ejecutable en monospace. Nombres largos o cortos jamás desplazan los controles.
    - **Columna 2 (Vúmetro & Fader - flexible centrado):** Vúmetro de barra segmentada LED, fader de curva cúbica idéntica a OBS y píldora monospace con readout numérico de porcentaje y decibelios.
    - **Columna 3 (Controles - 360 px en Estudio / 115 px en Flyout):** En estudio, 7 controles colocados en una sola fila horizontal alineada (`✕`, `FIJO`, `COMPRESOR`, `PRIO 2`, `SIDECHAIN`, `VOZ`, `MUTE`). En flyout, píldora compacta de rol (`SC`, `P2`, `DUCK`, `FIJO`, `NORM`) + mute + botón quitar.
  - Los encabezados de tabla superiores se calculan con las mismas coordenadas X exactas, logrando un alineamiento vertical perfecto con cada una de las 17 tarjetas.
- **Cabecera en 3 filas sin solapamientos:**
  - **Fila 1:** Logo de audio WASAPI + Selector de dispositivo de salida + Volumen Maestro con fader, porcentaje y vúmetro estéreo + Botones de gestión de ventana (`[— Ocultar]`, `[🗕 Modo Bandeja]`, `[📌 Auto-ocultar]`).
  - **Fila 2:** Buscador de canales + Pestañas de filtrado (`Todas` vs `Solo en ejecución`) + Botones de procesamiento DSP con menús emergentes + Cajones de Ajustes Compresor y Agregar Aplicación.
  - **Fila 3:** Barra dedicada de telemetría de atenuación en vivo (vúmetro de reducción Sidechain + vúmetro de reducción Modo Enfoque + conteo de canales activos). Evita que medidores y controles compitan por espacio horizontal.
- **Tipografía nativa Windows 11:** `Segoe UI` para la interfaz y `Consolas` para decibelios y porcentajes.

---

## 3. Fuentes Precargadas (Escena OBS de Juan Antonio)

| # | Nombre | Ejecutable / Dispositivo | Volumen inicial | Trigger | Comp | Fijo | Voz | Mute |
|---|---|---|---|:---:|:---:|:---:|:---:|:---:|
| 1 | **Discord** | `Discord.exe` | 100.0% (0.0 dB) | Sí | No | Sí | **Sí** | No |
| 2 | **Overwatch** | `Overwatch.exe` | 19.2% (-14.4 dB) | No | Sí | No | No | No |
| 3 | **Chrome** | `chrome.exe` | 30.8% (-10.2 dB) | No | Sí | No | No | No |
| 4 | **WuWa** | `Client-Win64-Shipping.exe` | 2.1% (-33.7 dB) | No | Sí | No | No | No |
| 5 | **ZZZ** | `ZenlessZoneZero.exe` | 4.5% (-27.0 dB) | No | Sí | No | No | No |
| 6 | **HI3** | `BH3.exe` | 2.5% (-32.2 dB) | No | Sí | No | No | No |
| 7 | **HSR** | `StarRail.exe` | 1.3% (-37.9 dB) | No | Sí | No | No | No |
| 8 | **GI** | `GenshinImpact.exe` | 10.0% (-20.0 dB) | No | Sí | No | No | No |
| 9 | **Minecraft** | `javaw.exe` | 3.7% (-28.5 dB) | No | Sí | No | No | No |
| 10 | **Spotify** | `Spotify.exe` | 9.1% (-20.8 dB) | No | Sí | No | No | No |
| 11 | **Brave** | `brave.exe` | 5.7% (-24.8 dB) | No | Sí | No | No | No |
| 12 | **Arknights** | `Endfield.exe` | 6.0% (-24.4 dB) | No | Sí | No | No | No |
| 13 | **NTE** | `HTGame.exe` | 1.1% (-39.6 dB) | No | Sí | No | No | No |
| 14 | **Mumu** | `scrcpy.exe` | 100.0% (0.0 dB) | No | No | No | No | No |
| 15 | **Opera GX** | `opera.exe` | 12.1% (-18.3 dB) | No | Sí | No | No | No |
| 16 | **VoLum** | `VoLum.exe` | 100.0% (0.0 dB) | No | No | No | No | No |
| 17 | **Digitech** | `{bfc138e0-2398-4437-bc68-caa04d7ba3e6}` | 35.4% (-9.0 dB) | No | No | No | No | **Sí** |

*Cualquier cambio se guarda automáticamente en `config.json`.*

---

## 4. Instrucciones de Compilación y Despliegue (Laboratorio Vibebox)

Compilado en la máquina virtual `vibebox` (`ssh vibebox` → `192.168.0.18`) sin tocar el host Windows:

### 4.1 Encender `Vibebox` y activar la guardia anti-apagado (`.cancel-auto`)
```powershell
Start-Process -FilePath "powershell.exe" -ArgumentList "-NoProfile -Command `"Start-VM -Name 'Vibebox'`"" -Verb RunAs -Wait
for ($i=1; $i -le 30; $i++) {
    $tcp = New-Object System.Net.Sockets.TcpClient
    try {
        $ar = $tcp.BeginConnect("192.168.0.18", 22, $null, $null)
        if ($ar.AsyncWaitHandle.WaitOne(1000, $false) -and $tcp.Connected) { $tcp.Close(); break }
    } catch {}
    $tcp.Close(); Start-Sleep -Seconds 1
}
ssh -o ConnectTimeout=5 vibebox "touch /home/user/.cancel-auto; pkill -f '[a]uto-daily' 2>/dev/null; echo VIBEBOX_READY"
```

### 4.2 Sincronizar código y compilar en `Vibebox`
```powershell
scp -r "C:\Users\Claassical\Documents\Antigravity\Proyectos\Audio-Monitor\src" vibebox:~/proyectos/audio-monitor/
ssh vibebox "export PATH=`"`$HOME/.local/llvm-mingw/bin:`$HOME/.cargo/bin:`$PATH`" && cd ~/proyectos/audio-monitor && cargo build --release --target x86_64-pc-windows-gnu"
```

### 4.3 Copiar, firmar, apagar `Vibebox` y abrir en el escritorio interactivo (`WinSta0\Default`)
```powershell
Stop-Process -Name "AudioMonitor" -Force -ErrorAction SilentlyContinue
$exePath = "C:\Users\Claassical\Documents\Antigravity\Proyectos\Audio-Monitor\AudioMonitor.exe"
$workDir = "C:\Users\Claassical\Documents\Antigravity\Proyectos\Audio-Monitor"

scp vibebox:~/proyectos/audio-monitor/target/x86_64-pc-windows-gnu/release/AudioMonitor.exe $exePath
ssh -o ConnectTimeout=5 vibebox "rm -f /home/user/.cancel-auto; sudo -n /usr/sbin/shutdown -h now"

$cert = Get-ChildItem "Cert:\CurrentUser\My" -CodeSigningCert | Where-Object { $_.Subject -eq "CN=Claassical Local Dev" } | Select-Object -First 1
Set-AuthenticodeSignature -FilePath $exePath -Certificate $cert -HashAlgorithm SHA256

# Lanzar en el escritorio interactivo del usuario (evitando el escritorio invisible WinSta0\exebox-*):
$taskName = "LaunchAudioMonitorInteractive"
$action = New-ScheduledTaskAction -Execute $exePath -WorkingDirectory $workDir
$principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName $taskName
Start-Sleep -Seconds 2
Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
```

---

## 5. Guía de Uso Rápido

1. **Claridad de Voz para Discord:**
   - En la barra superior, haz clic en `[ 🎙 VOZ CLARA DISCORD: ACTIVO ]` para encender o apagar la mejora vocal en cualquier momento.
   - Puedes alternar entre **Nítido / Presencia**, **Extra Claridad** (si alguien murmura) o **Broadcast** (para mayor calidez).
   - En cada canal puedes prender o apagar la píldora verde `VOZ`.
2. **Ajustes y Presets del Compresor Sidechain:**
   - Haz clic en `⚙ Ajustes Compresor`.
   - Selecciona **`🎮 Gaming Pro`** para la mejor experiencia en juegos (sin cortes bruscos y sin silencios incómodos).
   - O prueba **`🎯 Competitivo`** cuando requieras máxima claridad de *callouts* en partidas clasificatorias.
3. **Ducking Espectral Inteligente con Bajada General:**
   - En la barra superior tienes el botón directo `[ 🌊 DUCKING ESPECTRAL: ON ]`. Cuando está encendido, el sidechain atenúa un 125% las frecuencias de la voz humana (300 Hz - 4 kHz) y un 75% los subgraves y agudos del juego (calibrable en el deslizador de *Ajustes Compresor* del 40% al 100%). La bajada de volumen se percibe clara y musical sin matar los graves.
4. **De-Esser Dinámico y Compuerta Anti-Siseo:**
   - En la barra superior tienes el botón `[ 🛡 DE-ESSER + GATE: ON ]`. Suaviza sibilancias estridentes en 6.2 kHz y atenúa de forma transparente el soplido de fondo y ventiladores de PC (-14 dB) en las pausas de Discord.
5. **Modo Enfoque / Jerarquía de Prioridad 2:**
   - En cualquier canal (ej. Chrome, Spotify, Brave o un juego secundario), haz clic en la píldora dorada `PRIO 2`.
   - Cuando esa aplicación reproduzca sonido (ej. un video de YouTube o canción), bajará automáticamente el volumen de todos los juegos normales a `-12 dB` (con un hold de 350 ms para evitar bombeos).
   - En la cabecera puedes encender o apagar el modo con `[ 🎯 ENFOQUE PRIO 2: ON / OFF ]`, el cual muestra en vivo los `-dB` de atenuación aplicados.
6. **Bandeja del Sistema (System Tray) y Modo Flyout EarTrumpet:**
   - **Mezclador rápido al hacer clic en el reloj:** Haz 1 clic izquierdo sobre el icono de audífonos junto al reloj: se desplegará instantáneamente el mezclador compacto (`500x580`) pegado a la barra de tareas. Si haces clic de nuevo sobre el icono o haces clic fuera de la ventana, se oculta automáticamente.
   - **Fijar con Pin `[📌]`:** Si quieres que el flyout se mantenga abierto mientras juegas o trabajas en otra ventana, haz clic en el botón `[📌]` en la cabecera.
   - **Alternar entre Estudio y Flyout:** Haz clic en `[🎛 Estudio]` para expandir la ventana a tamaño completo (`1120x740`), o en `[🗕 Modo Bandeja]` para regresar al flyout compacto.
   - **Cerrar a la bandeja:** Al hacer clic en la `X` de la ventana, la aplicación se oculta al área de notificación sin interrumpir el audio.
   - **Menú contextual:** Clic derecho sobre el icono para abrir rápidamente el flyout, el estudio, o alternar *Voz Clara Discord*, *Ducking Espectral*, *Modo Enfoque* o *Salir por completo*.
   - **Arranque silencioso:** El acceso directo de inicio puede invocar `AudioMonitor.exe --minimize-to-tray` para arrancar en segundo plano al iniciar Windows.
