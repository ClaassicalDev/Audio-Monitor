# AGENTS.md — Guía Técnica e Invariantes para Agentes de IA (`Audio-Monitor`)

Este documento contiene el contexto completo, arquitectura e **invariantes obligatorios** para que cualquier agente de IA (**Antigravity**, **Claude Code**, **Codex**, **Cursor**, **Windsurf**, etc.) continúe el desarrollo de **`AudioMonitor`** sin romper la estabilidad del sistema ni reintroducir bugs ya resueltos.

---

## 1. Propósito del Proyecto

`AudioMonitor.exe` es un mezclador y monitor de audio nativo para Windows 10/11 en Rust que reemplaza a **OBS Studio** como monitor de audio personal:
- Captura el audio de cada aplicación de forma independiente mediante **WASAPI Process Loopback (`VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK`)** con `PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE`, además de entradas USB físicas (`DigiTech RP500`).
- Aplica curvas de volumen cúbicas idénticas a OBS ($v = s^3$), **Claridad Vocal DSP para Discord**, **Ducking Espectral Híbrido** (`125%` en medios vocales `300 Hz – 4 kHz` y `75%` en el resto del espectro) y **Modo Enfoque (`Prioridad 2`)** exclusivamente en la salida de audífonos del usuario.
- De esta forma, cuando el usuario comparte pantalla en **Discord Screen Share**, los espectadores escuchan el juego al 100% de su volumen original sin que se baje cuando alguien habla en Discord.

---

## 2. Invariantes Críticos de Arquitectura (PROHIBIDO ROMPER)

### Invariante 1: Seguridad Anti-Cheat (0% Ban Risk)
El usuario juega títulos con sistemas Anti-Cheat de nivel kernel (**ACE / Anti-Cheat Expert** en *Wuthering Waves* y *NTE*; **HoYoKProtect** en *Genshin Impact*, *Honkai: Star Rail*, *Zenless Zone Zero*, *Honkai Impact 3rd*; *Arknights: Endfield*; *Overwatch 2*).
- **PROHIBIDO** llamar a `OpenProcess`, `ReadProcessMemory`, `WriteProcessMemory` o `VirtualAllocEx` sobre procesos de terceros.
- **PROHIBIDO** instalar hooks globales (`SetWindowsHookEx` con `WH_KEYBOARD_LL` o `WH_MOUSE_LL`), usar `RegisterHotKey` o hacer bucles de polling con `GetAsyncKeyState`.
- **PROHIBIDO** usar interfaces COM no documentadas como `IPolicyConfig` / `IAudioPolicyConfig` para forzar el auto-ruteo de procesos de juegos.
- **Mecanismos permitidos (100% seguros):**
  - `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)` + `Process32FirstW` / `Process32NextW` en `src/audio/process.rs`.
  - `EnumWindows` + `GetWindowThreadProcessId` + `GetWindowTextW` en `src/audio/process.rs`.
  - `ActivateAudioInterfaceAsync("VAD\\Process_Loopback", ...)` en `src/audio/capture.rs` (ejecutado por el servicio del sistema `audiodg.exe`).

### Invariante 2: Prevención de Deadlocks con `explorer.exe` en el System Tray (`src/tray.rs`)
En Windows 11, cuando el usuario hace clic sobre un icono del System Tray (`Shell_TrayWnd`, perteneciente a `explorer.exe`), `explorer.exe` envía `WM_TRAY_CALLBACK` a `tray_wnd_proc` (hilo `system-tray`) y bloquea el hilo de la barra de tareas esperando su retorno.
- **PROHIBIDO** llamar a `SetWindowPos`, `ShowWindow`, `BringWindowToTop` o `SetForegroundWindow` sobre `main_hwnd` (que pertenece al hilo principal de `winit`) desde dentro de `tray_wnd_proc` o de forma síncrona desde un hilo secundario. Hacerlo provoca un `SendMessage` síncrono entre hilos dentro de `win32kfull.sys` que congela a `explorer.exe`.
- **Arquitectura obligatoria implementada en `src/tray.rs` y `src/ui/app.rs`:**
  1. `tray_wnd_proc` en `WM_LBUTTONUP` / `WM_LBUTTONDBLCLK` / `WM_COMMAND` **únicamente** escribe un byte atómico en `GLOBAL_PENDING_ACTION: AtomicU8` (`ACTION_TOGGLE_FLYOUT`, `ACTION_OPEN_STUDIO`, `ACTION_OPEN_FLYOUT`) y retorna `LRESULT(0)` de inmediato sin bloquear ningún `Mutex` ni tocar `main_hwnd`.
  2. El hilo secundario `tray-worker` lee `GLOBAL_PENDING_ACTION`, actualiza las banderas atómicas (`pending_reposition`, `flyout_mode`, `auto_hide_enabled`, `last_shown_ms`) y despierta/oculta la ventana usando **exclusivamente** llamadas asíncronas: `ShowWindowAsync(hwnd, SW_SHOW / SW_HIDE)` + `InvalidateRect` + `PostMessageW(Some(hwnd), WM_NULL, ...)`.
  3. El posicionamiento y redimensionado (`position_window_flyout_on_owner_thread` y `position_window_studio_on_owner_thread`, que invocan `SetWindowPos`) se ejecutan **única y exclusivamente desde `AudioMonitorApp::ui()` en el hilo principal dueño de la ventana (`winit`)**.

### Invariante 3: Arranque Minimizado en Bandeja e Inicialización de OpenGL (`src/main.rs` y `src/ui/app.rs`)
- **PROHIBIDO** pasar `.with_visible(false)` o coordenadas fuera de pantalla (`[-32000.0, -32000.0]`) en `ViewportBuilder`, o llamar a `ShowWindow(hwnd, SW_HIDE)` dentro de `AudioMonitorApp::new` antes de que `eframe` / `glutin` / `nvoglv64.dll` complete el primer frame de OpenGL.
- **Solución implementada:** La ventana inicia normalmente en el Frame 0 para que el driver WGL inicialice el swapchain sin bloquearse, y en el Frame 1 (`self.startup_frames == 2` dentro de `AudioMonitorApp::ui()`), si el usuario no ha solicitado abrirla (`last_shown_ms == 0`), el propio hilo principal ejecuta `ShowWindow(hwnd, SW_HIDE)`.

### Invariante 4: Estado por Defecto de Vúmetros (`ChannelLiveStatus` en `src/audio/renderer.rs`)
- `ChannelLiveStatus::default()` **debe** inicializar `vu_peak_db: -100.0` (silencio / barra vacía), **nunca** `0.0` (`#[derive(Default)]` en `f32` ponía `0.0 dB`, mostrando la barra llena al 100% en rojo en canales muteados al arrancar).
- En el bucle del mezclador (`renderer.rs`), cualquier canal con `ch.muted == true` o `!status.active` debe forzar `status.vu_peak_db = -100.0`.
- Para entradas físicas (`__hardware_input__` / `Digitech`), `CaptureWorker::is_hardware_input_connected` verifica primero si el dispositivo de captura USB existe antes de marcar `status.active = true`.

### Invariante 5: Geometría Matemática sin Solapamientos en la UI (`src/ui/app.rs`)
- **Cabecera Fila 1:** Dividida explícitamente en `row1_left_rect` (Logo + `Salida:` con texto truncado a 26 caracteres + `Maestro:`) y `row1_right_rect` (`290 px` fijos a la derecha para `[— Ocultar a Bandeja]` y `[Modo Bandeja (Popup)]`).
- **Filas de Canales:** Cada tarjeta pre-asigna su rectángulo con `allocate_exact_size` (`row_h = 56.0` en Estudio, `44.0` en Flyout) y lo divide con `Rect::from_min_max` + `ui.set_clip_rect`:
  - **Col 1 (`220 px` Estudio / `130 px` Flyout):** Diseño limpio de **2 renglones** (`[● LED] Nombre` arriba + `ESTADO · Ejecutable.exe` abajo) con `.truncate()`.
  - **Col 2 (Flexible central):** Vúmetro segmentado LED + Fader cúbico + píldora monospace `% | dB`.
  - **Col 3 (`396 px` Estudio / `128 px` Flyout):** Con `item_spacing.x = 6.0` para que los 7 botones (`✕`, `FIJO`, `COMPRESOR`, `PRIO 2`, `SIDECHAIN`, `VOZ`, `MUTE`) midan `376 px` y el botón de `MUTE` (`34×28 px`) jamás quede recortado.
- **Tipografía:** Se cargan `segoeui.ttf`, `seguisym.ttf` y `consola.ttf`. Evitar emojis de color no soportados por `segoeui.ttf` (usar etiquetas de texto limpias para evitar cuadros `□`).

---

## 3. Mapa de Archivos Clave

| Archivo | Responsabilidad |
|---|---|
| `src/main.rs` | Configuración de fuentes (`Segoe UI`, `Segoe UI Symbol`, `Consolas`), icono 32x32 y arranque de `eframe`. |
| `src/config.rs` | Modelos JSON (`AppConfig`, `ChannelConfig`, `CompressorConfig`), 17 fuentes por defecto y 4 presets de compresor. |
| `src/tray.rs` | Icono Win32 en bandeja (`Shell_NotifyIconW`), `GLOBAL_PENDING_ACTION` lock-free, `tray-worker` asíncrono (`ShowWindowAsync`) y helpers `SetWindowPos` para el hilo dueño. |
| `src/audio/capture.rs` | Captura `WASAPI Process Loopback` por árbol de PID y captura de entrada física USB (`DigiTech`). |
| `src/audio/process.rs` | Enumeración de procesos sin `OpenProcess` (`CreateToolhelp32Snapshot`) y resolución de títulos de ventana (`EnumWindows`). |
| `src/audio/dsp.rs` | Curva cúbica OBS, `VoiceClarityProcessor` (HPF, Gate, EQ, De-Esser, Leveler), `SpectralDuckingFilter` (125% vocal / 75% general), `SidechainCompressor` y `FocusDucker` (Prioridad 2). |
| `src/audio/renderer.rs` | Hilo mezclador a `48,000 Hz` float32 estéreo, seguimiento del dispositivo por defecto y cálculo de picos dB. |
| `src/audio/ring_buffer.rs` | Cola circular estéreo thread-safe entre los hilos de captura y el mezclador. |
| `src/ui/app.rs` | Estado de la UI, modo Estudio (`1140×740`) y modo Flyout EarTrumpet (`540×600`), cajones de Compresor y Agregar App. |
| `src/ui/widgets.rs` | Componentes vectoriales hechos a medida: `studio_segmented_vu_meter`, `gain_reduction_meter`, `vector_mute_button`, `pill_toggle_button`, `draw_led_dot`. |

---

## 4. Flujo de Compilación y Despliegue en el Entorno de Juan Antonio (`Claassical`)

> **Regla de Host Limpio:** En la PC principal con Windows 11 Pro **no** se instalan compiladores ni toolchains globales. Toda compilación se realiza por SSH en la VM laboratorio **`vibebox`** (`192.168.0.18`).

### Paso 1: Encender `vibebox` y desactivar el auto-apagado del cron `@reboot`
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

### Paso 2: Sincronizar `src/` y compilar `--release` (`x86_64-pc-windows-gnu`)
```powershell
scp -r "C:\Users\Claassical\Documents\Antigravity\Proyectos\Audio-Monitor\src" vibebox:~/proyectos/audio-monitor/
ssh vibebox "export PATH=`"`$HOME/.local/llvm-mingw/bin:`$HOME/.cargo/bin:`$PATH`" && cd ~/proyectos/audio-monitor && cargo build --release --target x86_64-pc-windows-gnu"
```

### Paso 3: Copiar el binario, apagar `vibebox`, firmar con SHA-256 y lanzar en `WinSta0\Default`
```powershell
Stop-Process -Name "AudioMonitor" -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 400

$exePath = "C:\Users\Claassical\Documents\Antigravity\Proyectos\Audio-Monitor\AudioMonitor.exe"
$workDir = "C:\Users\Claassical\Documents\Antigravity\Proyectos\Audio-Monitor"

scp vibebox:~/proyectos/audio-monitor/target/x86_64-pc-windows-gnu/release/AudioMonitor.exe $exePath
ssh -o ConnectTimeout=5 vibebox "rm -f /home/user/.cancel-auto; sudo -n /usr/sbin/shutdown -h now"

$cert = Get-ChildItem "Cert:\CurrentUser\My" -CodeSigningCert | Where-Object { $_.Subject -eq "CN=Claassical Local Dev" } | Select-Object -First 1
Set-AuthenticodeSignature -FilePath $exePath -Certificate $cert -HashAlgorithm SHA256

# Lanzar en el escritorio interactivo real del usuario (WinSta0\Default):
$taskName = "LaunchAudioMonitorInteractive"
$action = New-ScheduledTaskAction -Execute $exePath -WorkingDirectory $workDir
$principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName $taskName
Start-Sleep -Seconds 2
Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
```
