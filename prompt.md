Cómo me imagino el rediseño
CONCEPTO VISUAL · CONTROL CENTER

roadwaves
Your world. Your radio.

 Live



NOW PLAYING · FRANCE
Radio Nova
Paris, France · Electronic / Indie








Auto discovery

Find radio by location


Game overlay

Ctrl + Alt + R




GitHub · Source code

Open source


Prototipo visual, no una implementación conectada al servicio. El botón de reproducción y el volumen simulan controles de la interfaz.HUD encima del juego
Lo haría más compacto, con modo click-through para no interferir con el mouse. No mostraría todo el panel de configuración durante la conducción.

● LIVE · FRANCE
Radio Nova
Paris, France



CTRL + ALT + R · HIDE


Los controles se habilitarían en modo interactivo; en modo conducción serían solo indicadores. También le agregaría una animación de entrada de 250 ms y un modo minimalista que muestre la emisora durante unos segundos al cambiar de estación.
Stack que usaría
Componente	Tecnología
HUD nativo	egui + eframe
Atajos globales	global-hotkey
Icono en bandeja del sistema	tray-icon
Servicio de radio	Rust, reutilizando el core de RoadWaves
Estado compartido	tokio::sync::watch o canales
Configuración persistente	serde + TOML
eframe permite ventanas transparentes, sin bordes, always-on-top y con eventos de mouse que atraviesan la ventana. Está soportado directamente por su API de viewports. 

Rust
+1



Código base del overlay
Creás el proyecto e instalás las dependencias:
cargo new roadwaves-hud
cd roadwaves-hud
cargo add eframe global-hotkey


El núcleo de la configuración de ventana sería:
use eframe::egui;

let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default()
        .with_title("RoadWaves HUD")
        .with_inner_size([420.0, 140.0])
        .with_decorations(false)
        .with_resizable(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_mouse_passthrough(true),
    ..Default::default()
};


Después registrás el atajo:
use global_hotkey::{
    GlobalHotKeyManager,
    hotkey::{HotKey, Modifiers, Code},
};

let manager = GlobalHotKeyManager::new()?;

let toggle = HotKey::new(
    Some(Modifiers::CONTROL | Modifiers::ALT),
    Code::KeyR,
);

manager.register(toggle)?;


Y procesás el evento desde el loop de la aplicación, conservando manager durante toda su vida:
use global_hotkey::{
    GlobalHotKeyEvent,
    HotKeyState,
};

while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
    if event.id == toggle_id
        && event.state == HotKeyState::Pressed
    {
        hud_visible = !hud_visible;
    }
}


Para ocultarlo, inicialmente mantendría la ventana transparente y en click-through, dejando de dibujar el contenido cuando hud_visible sea false. Así el loop puede seguir procesando el atajo que vuelve a mostrarlo. También separaría el HUD del reproductor para que ocultar o cerrar la ventana nunca detenga la radio.
Esto es el esqueleto, no una aplicación completa compilada.
Compatibilidad real con ETS2 y ATS
Hay una diferencia importante entre un overlay de escritorio y uno integrado al renderizado del juego.
Plataforma	Enfoque
Windows 10/11	Ventana transparente above-game, preferentemente con el juego en borderless
Ubuntu/Fedora con X11	Ventana always-on-top + global hotkey
Fedora/Ubuntu con Wayland	Backend especial para overlay y shortcuts
Fullscreen exclusivo	Puede requerir overlay integrado al pipeline gráfico
La librería global-hotkey soporta Linux con X11, no Wayland. Para Wayland existe el portal de atajos globales, pero depende de lo que implemente el escritorio. 

Rust
+1



Como querés que RoadWaves funcione en Windows, Fedora y Ubuntu, diseñaría la interfaz de overlay detrás de un trait como OverlayBackend, para tener una implementación Windows/X11 y otra específica de Wayland sin acoplar el reproductor.
Cómo estructuraría RoadWaves
roadwaves/
├── crates/
│   ├── roadwaves-core/
│   │   ├── radio/
│   │   ├── playback/
│   │   └── discovery/
│   ├── roadwaves-hud/
│   │   ├── overlay/
│   │   ├── hotkeys/
│   │   └── widgets/
│   ├── roadwaves-desktop/
│   │   ├── player/
│   │   ├── settings/
│   │   └── about/
│   └── roadwaves-platform/
│       ├── windows/
│       └── linux/
└── Cargo.toml


En la pantalla About, además del footer del Control Center, pondría el repo tomas-santucho/roadwaves, la versión instalada, el estado del servicio y un enlace a GitHub Issues para reportar bugs.
Mi elección para el MVP: eframe + egui para mantener todo en Rust. Si después queremos una interfaz de escritorio todavía más premium, se puede construir el Control Center con Tauri/Svelte y conservar el HUD nativo sin tocar el core.
Y probaría el overlay primero con ETS2 en modo borderless, antes de avanzar con todas las animaciones. Es la forma de validar el punto más complicado: que realmente quede visible sobre el juego y no se coma los controles.
