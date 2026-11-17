//! The eframe web runner: canvas lookup, backend selection, the loading screen's startup steps,
//! browser shortcuts the app takes over, the unsaved-changes guard on close, and startup errors
//! shown in the page.

use texturecraft_engine::Session;
use texturecraft_ui_egui::TextureCraftApp;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::closure::Closure;

const CANVAS_ID: &str = "texturecraft_canvas";
const LOADING_ID: &str = "texturecraft_loading";
const STATUS_ID: &str = "texturecraft_status";
const PROGRESS_ID: &str = "texturecraft_progress";
const BAR_ID: &str = "tc_load_bar";
const PCT_ID: &str = "tc_load_pct";

pub fn start() {
    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    capture_app_shortcuts();
    guard_unsaved_changes();
    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            log::error!("no document");
            return;
        };
        let result = match document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) {
            Some(canvas) => run(canvas).await,
            None => Err(format!("the page has no <canvas id=\"{CANVAS_ID}\">")),
        };
        if let Some(el) = document.get_element_by_id(LOADING_ID) {
            match result {
                Ok(()) => el.remove(),
                Err(e) => {
                    log::error!("The AllCraft Texture failed to start: {e}");
                    el.set_text_content(Some(&format!(
                        "The AllCraft Texture failed to start: {e}. It needs a browser with WebGPU (current Chrome, Edge or Safari) or at least WebGL2."
                    )));
                }
            }
        }
    });
}

async fn run(canvas: web_sys::HtmlCanvasElement) -> Result<(), String> {
    step(0.90, "Preparing sample 3D project & baked maps…").await;
    let session = initial_session()?;
    step(0.98, "Starting 3D PBR & UV renderer…").await;
    let mut options = eframe::WebOptions::default();
    if query().contains("webgl")
        && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
    {
        create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
    }
    eframe::WebRunner::new()
        .start(
            canvas,
            options,
            Box::new(|cc| {
                let mut app = TextureCraftApp::new(cc, session);
                app.ui.windows.start = app.ui.prefs.start_window;
                if let Some(rs) = &cc.wgpu_render_state {
                    let backend = rs.adapter.get_info().backend;
                    log::info!("The AllCraft Texture: wgpu backend {backend:?}");
                    if backend == eframe::wgpu::Backend::Gl {
                        app.banner = Some(texturecraft_ui_egui::WEBGL_NOTICE);
                    }
                }
                Ok(Box::new(app))
            }),
        )
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))
}

async fn step(progress: f32, status: &str) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    if let Some(el) = document.get_element_by_id(STATUS_ID) {
        el.set_text_content(Some(status));
    }
    if let Some(el) = document.get_element_by_id(PROGRESS_ID) {
        let _ = el.set_attribute("value", &progress.to_string());
    }
    let pct = (progress * 100.0).round() as u32;
    if let Some(bar) = document.get_element_by_id(BAR_ID).and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
        let _ = bar.style().set_property("width", &format!("{pct}%"));
    }
    if let Some(pct_el) = document.get_element_by_id(PCT_ID) {
        pct_el.set_text_content(Some(&format!("{pct}%")));
    }
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 20);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

fn capture_app_shortcuts() {
    let Some(window) = web_sys::window() else { return };
    let on_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|e: web_sys::KeyboardEvent| {
        let key = eframe::egui::Key::from_name(&e.key()).or_else(|| eframe::egui::Key::from_name(&e.code()));
        let Some(key) = key else { return };
        let mods = eframe::egui::Modifiers {
            alt: e.alt_key(),
            ctrl: e.ctrl_key(),
            shift: e.shift_key(),
            mac_cmd: e.meta_key(),
            command: e.ctrl_key() || e.meta_key(),
        };
        if texturecraft_ui_egui::keymap::captured(mods, key) {
            e.prevent_default();
        }
    });
    let options = web_sys::AddEventListenerOptions::new();
    options.set_capture(true);
    if let Err(e) = window.add_event_listener_with_callback_and_add_event_listener_options("keydown", on_key.as_ref().unchecked_ref(), &options) {
        log::warn!("couldn't take over the browser's shortcuts: {e:?}");
    }
    on_key.forget();
}

fn guard_unsaved_changes() {
    let Some(window) = web_sys::window() else { return };
    let on_unload = Closure::<dyn FnMut(web_sys::BeforeUnloadEvent)>::new(|e: web_sys::BeforeUnloadEvent| {
        if texturecraft_ui_egui::web_files::unsaved() {
            e.prevent_default();
            e.set_return_value("unsaved");
        }
    });
    if let Err(e) = window.add_event_listener_with_callback("beforeunload", on_unload.as_ref().unchecked_ref()) {
        log::warn!("couldn't guard unsaved changes on close: {e:?}");
    }
    on_unload.forget();
}

fn initial_session() -> Result<Session, String> {
    texturecraft_engine::sample_session(1024, 128)
        .or_else(|e| {
            log::warn!("sample project failed: {e}");
            Session::new_project("Untitled", texturecraft_engine::geom::primitives::material_ball(), "builtin:material_ball", None, 1024)
        })
        .map_err(|e| e.to_string())
}

fn query() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}
