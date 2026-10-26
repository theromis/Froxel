// Froxel editor.
//
// ```sh
// cargo run
// cargo run -- path/to/model.vox
// cargo run -- --control 7979
// cargo run -- mcp
// cargo run -- mcp --connect 127.0.0.1:7979 --launch
// ```
//
// `mcp` speaks Model Context Protocol on stdin/stdout. Without `--connect`
// the session is in-process and has no window. `--connect` drives an editor
// that was started with `--control`, which is where screenshots come from.
// `--launch` opens that editor. `froxel bridge` and `--bridge` speak the
// Godot/Blender bridge. Render mode path-traces on the GPU through wgpu.
// MCP `render` and `froxel render` path-trace a PNG with no window.
// `froxel voxrt` checks a `.vox` round trip.
//
// On macOS the File and Edit commands live on the application menu bar.
//
// This file is the desktop binary crate and, via `include!` from `src/lib.rs`,
// the Android cdylib. Inner doc comments cannot be included that way.

mod bridge;
mod cli;
mod control;
mod denoise;
mod export;
mod gpu;
mod icons;
mod mcp;
mod menu;
mod menu_bar;
mod mesh;
mod mobile;
#[cfg(target_os = "macos")]
mod native_menu;
mod panels;
mod remote;
mod rig;
mod script;
mod state;
mod still;
mod tools;
mod trace;
mod trace_gpu;
mod viewport;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use egui::{Color32, RichText, Visuals};
use serde_json::Value;

use control::ControlServer;
use mcp::ShotRegion;
use state::{Session, UiAction};
use viewport::ViewCache;

/// Appends a line to `Documents/froxel.log` on a phone, which has no console.
#[cfg(any(target_os = "ios", target_os = "android"))]
pub(crate) fn append_log(line: &str) {
    use std::io::Write;
    let path = state::documents_dir().join("froxel.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// Sends eframe's and wgpu's log records to `Documents/froxel.log` on a
/// phone, where the app has no console.
#[cfg(any(target_os = "ios", target_os = "android"))]
struct FileLog;

#[cfg(any(target_os = "ios", target_os = "android"))]
impl log::Log for FileLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            append_log(&format!(
                "{} {}: {}",
                record.level(),
                record.target(),
                record.args()
            ));
        }
    }
    fn flush(&self) {}
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    let _ = main(app);
}

fn main(
    #[cfg(target_os = "android")] app: winit::platform::android::activity::AndroidApp,
) -> eframe::Result {
    #[cfg(any(target_os = "ios", target_os = "android"))]
    {
        static LOGGER: FileLog = FileLog;
        let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Warn));
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "mcp") {
        std::process::exit(mcp::run(&args[1..]));
    }
    if args.first().is_some_and(|arg| arg == "render") {
        std::process::exit(cli::render(&args[1..]));
    }
    if args.first().is_some_and(|arg| arg == "voxrt") {
        std::process::exit(cli::voxrt(&args[1..]));
    }
    if args.first().is_some_and(|arg| arg == "bridge") {
        std::process::exit(bridge::run(&args[1..]));
    }
    let gui = match parse_gui(&args) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        }
    };
    let control = match gui.control {
        Some(port) => match ControlServer::start(port) {
            Ok(server) => Some(server),
            Err(err) => {
                eprintln!("control port {port}: {err}");
                std::process::exit(1);
            }
        },
        None => None,
    };
    let bridge = match gui.bridge {
        Some(port) => match bridge::BridgeServer::start(port) {
            Ok(server) => Some(server),
            Err(err) => {
                eprintln!("bridge port {port}: {err}");
                std::process::exit(1);
            }
        },
        None => None,
    };
    #[cfg(target_os = "macos")]
    if menu::native() {
        native_menu::suppress_system_edit_items();
    }
    let mut session = Session::new();
    if let Some(path) = gui.file {
        session.load_path(path.as_path());
    }

    // The phone strip is what `FROXEL_MOBILE_UI` is for. The desktop minimum
    // is wider than that strip, so a preview starts at a phone size.
    let (inner, min_inner) = if mobile::active() {
        ([390.0, 844.0], [320.0, 480.0])
    } else {
        ([1280.0, 800.0], [960.0, 640.0])
    };
    #[cfg_attr(not(any(target_os = "ios", target_os = "android")), allow(unused_mut))]
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(inner)
            .with_min_inner_size(min_inner)
            .with_title("Froxel")
            .with_icon(window_icon()),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    #[cfg(any(target_os = "ios", target_os = "android"))]
    mobile_options(&mut options);
    #[cfg(target_os = "android")]
    {
        options.android_app = Some(app);
    }
    #[cfg(any(target_os = "ios", target_os = "android"))]
    std::panic::set_hook(Box::new(|info| {
        append_log(&format!("panic: {info}"));
    }));
    let result = eframe::run_native(
        "Froxel",
        options,
        Box::new(move |cc| {
            apply_theme(&cc.egui_ctx);
            Ok(Box::new(Shell {
                session,
                cache: ViewCache::new(),
                control,
                bridge,
                shots: VecDeque::new(),
                inflight: None,
                viewport_rect: egui::Rect::NOTHING,
                mobile: mobile::MobileUi::default(),
                #[cfg(target_os = "macos")]
                native_menu: None,
            }))
        }),
    );
    #[cfg(any(target_os = "ios", target_os = "android"))]
    if let Err(err) = &result {
        append_log(&format!("eframe: {err}"));
    }
    result
}

/// A phone fills its screen, and its GPU may fall short of wgpu's default
/// limits (the iOS simulator allows 15 inter-stage variables, not 16), so the
/// device asks for what the adapter has.
#[cfg(any(target_os = "ios", target_os = "android"))]
fn mobile_options(options: &mut eframe::NativeOptions) {
    use eframe::egui_wgpu::{WgpuSetup, WgpuSetupCreateNew};
    use std::sync::Arc;

    options.viewport = egui::ViewportBuilder::default().with_title("Froxel");
    options.wgpu_options.wgpu_setup = WgpuSetup::CreateNew(WgpuSetupCreateNew {
        device_descriptor: Arc::new(|adapter| eframe::wgpu::DeviceDescriptor {
            label: Some("froxel device"),
            required_limits: adapter.limits(),
            ..Default::default()
        }),
        ..WgpuSetupCreateNew::without_display_handle()
    });
}

/// The voxel frog of `packaging/icons`. macOS gets the padded tile its Dock expects.
fn window_icon() -> egui::IconData {
    #[cfg(target_os = "macos")]
    let png = include_bytes!("../packaging/icons/froxel-macos-256.png");
    #[cfg(not(target_os = "macos"))]
    let png = include_bytes!("../packaging/icons/hicolor/256x256.png");
    eframe::icon_data::from_png_bytes(png).expect("the bundled icon is a valid PNG")
}

struct Shell {
    session: Session,
    cache: ViewCache,
    control: Option<ControlServer>,
    bridge: Option<bridge::BridgeServer>,
    shots: VecDeque<PendingShot>,
    inflight: Option<InflightShot>,
    viewport_rect: egui::Rect,
    mobile: mobile::MobileUi,
    #[cfg(target_os = "macos")]
    native_menu: Option<native_menu::NativeMenu>,
}

struct PendingShot {
    region: ShotRegion,
    path: Option<String>,
    reply: Sender<Result<Value, String>>,
}

struct InflightShot {
    /// Filled by the screenshot callback once the frame has been read back.
    image: Arc<Mutex<Option<Arc<egui::ColorImage>>>>,
    region: ShotRegion,
    path: Option<String>,
    reply: Sender<Result<Value, String>>,
    pixels_per_point: f32,
    viewport: egui::Rect,
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        if let Some(control) = &self.control {
            control.bind_context(ctx);
        }
        if let Some(bridge) = &self.bridge {
            bridge.bind_context(ctx);
        }
        let bridge_path = self
            .bridge
            .as_ref()
            .map(|bridge| bridge.session_path.display().to_string());
        self.take_screenshot();
        self.take_control();
        self.take_bridge();
        self.session.clear_ui();
        let screen = ctx.input(|input| input.content_rect());
        self.session
            .note_ui("window", "Window", screen, UiAction::None);
        #[cfg(target_os = "macos")]
        {
            if self.native_menu.is_none() && menu::native() {
                self.native_menu = Some(native_menu::NativeMenu::install());
            }
            if let Some(menu) = &mut self.native_menu {
                menu.poll(&mut self.session, ctx);
            }
        }
        if ctx.input(|input| input.viewport().close_requested()) && !self.session.confirm_close() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }

        let title = format!("Froxel — {}", self.session.document_label());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        self.session.take_drops(ctx);
        self.session.take_keys(ctx);

        if mobile::active() {
            self.viewport_rect = mobile::show(
                ui,
                frame.wgpu_render_state(),
                &mut self.session,
                &mut self.cache,
                &mut self.mobile,
            );
            self.issue_screenshot(ctx);
            return;
        }

        menu_bar::show(ui, &mut self.session);

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&self.session.status)
                        .small()
                        .color(Color32::from_rgb(196, 206, 220)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let n = self.session.voxel_count();
                    ui.monospace(format!("{n} voxels"));
                    if let Some(path) = &bridge_path {
                        ui.monospace(
                            RichText::new(format!("bridge {path}"))
                                .small()
                                .color(Color32::from_rgb(160, 176, 196)),
                        );
                    }
                });
            });
        });

        egui::Panel::left("tools")
            .resizable(true)
            .default_size(220.0)
            .size_range(200.0..=300.0)
            .frame(panels::side_frame(&ctx.global_style()))
            .show(ui, |ui| panels::left(ui, &mut self.session));

        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(300.0)
            .size_range(280.0..=400.0)
            .frame(panels::side_frame(&ctx.global_style()))
            .show(ui, |ui| panels::right(ui, &mut self.session));

        egui::CentralPanel::default().show(ui, |ui| {
            self.viewport_rect = viewport::show(
                ctx,
                ui,
                frame.wgpu_render_state(),
                &mut self.session,
                &mut self.cache,
                true,
                &[],
            );
        });
        self.issue_screenshot(ctx);
    }
}

impl Shell {
    fn take_control(&mut self) {
        let Some(control) = &self.control else {
            return;
        };
        for job in control.poll() {
            if job.method == "ui.screenshot" {
                match mcp::parse_shot(&job.params) {
                    Ok((region, path)) => self.shots.push_back(PendingShot {
                        region,
                        path,
                        reply: job.reply,
                    }),
                    Err(err) => {
                        let _ = job.reply.send(Err(err));
                    }
                }
            } else if job.method == "ui.inspect" {
                let _ = job.reply.send(Ok(self.session.describe_ui()));
            } else if job.method == "ui.click" {
                let _ = job.reply.send(self.click_ui(&job.params));
            } else {
                let result = remote::apply(&mut self.session, &job.method, &job.params);
                let _ = job.reply.send(result);
            }
        }
    }

    fn take_bridge(&mut self) {
        let Some(bridge) = &self.bridge else {
            return;
        };
        let jobs = bridge.poll();
        let root = bridge.root.clone();
        for job in jobs {
            let result = bridge::dispatch(&mut self.session, &root, job.id, &job.op);
            let _ = job.reply.send(result);
        }
    }

    fn click_ui(&mut self, params: &Value) -> Result<Value, String> {
        let x = params
            .get("x")
            .and_then(Value::as_f64)
            .ok_or("missing number x")? as f32;
        let y = params
            .get("y")
            .and_then(Value::as_f64)
            .ok_or("missing number y")? as f32;
        let Some(region) = self.session.ui_target(x, y) else {
            return Err(format!("no control at {x}, {y}"));
        };
        if self.mobile.activate(&region.id, &mut self.session) {
            return Ok(serde_json::json!({"id": region.id, "label": region.label}));
        }
        if region.action == UiAction::Viewport {
            let rect = egui::Rect::from_min_size(
                egui::pos2(region.x, region.y),
                egui::vec2(region.w, region.h),
            );
            let ray = viewport::ray_through(&self.session, rect, egui::pos2(x, y));
            self.session.click_viewport(ray);
        } else {
            self.session.activate_ui(&region, Some((x, y)))?;
        }
        Ok(serde_json::json!({"id": region.id, "label": region.label}))
    }

    fn take_screenshot(&mut self) {
        let Some(shot) = &self.inflight else {
            return;
        };
        let Some(image) = shot.image.lock().ok().and_then(|mut slot| slot.take()) else {
            return;
        };
        let shot = self.inflight.take().expect("shot");
        let _ = shot.reply.send(finish_shot(&shot, &image));
    }

    fn issue_screenshot(&mut self, ctx: &egui::Context) {
        if self.inflight.is_some() {
            ctx.request_repaint();
            return;
        }
        let Some(shot) = self.shots.pop_front() else {
            return;
        };
        let image = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&image);
        let repaint = ctx.clone();
        ctx.request_screenshot(move |frame| {
            if let Ok(mut slot) = slot.lock() {
                *slot = Some(frame);
            }
            repaint.request_repaint();
        });
        self.inflight = Some(InflightShot {
            image,
            region: shot.region,
            path: shot.path,
            reply: shot.reply,
            pixels_per_point: ctx.pixels_per_point(),
            viewport: self.viewport_rect,
        });
        ctx.request_repaint();
    }
}

fn finish_shot(shot: &InflightShot, image: &egui::ColorImage) -> Result<Value, String> {
    let pixels: Vec<[u8; 4]> = image.pixels.iter().map(|color| color.to_array()).collect();
    let (width, height) = (image.size[0], image.size[1]);
    let region = match shot.region {
        ShotRegion::Window => None,
        ShotRegion::Viewport => Some(pixel_rect(
            shot.viewport,
            shot.pixels_per_point,
            width as i32,
            height as i32,
        )),
        ShotRegion::Rect {
            x,
            y,
            width: w,
            height: h,
        } => Some(pixel_rect(
            egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h)),
            shot.pixels_per_point,
            width as i32,
            height as i32,
        )),
    };
    let (png, w, h) = remote::png_region(&pixels, width, height, region)?;
    if let Some(path) = &shot.path {
        std::fs::write(path, &png).map_err(|err| format!("{path}: {err}"))?;
    }
    Ok(serde_json::json!({
        "width": w,
        "height": h,
        "path": shot.path,
        "pngBase64": remote::base64_encode(&png),
    }))
}

fn pixel_rect(
    rect: egui::Rect,
    pixels_per_point: f32,
    width: i32,
    height: i32,
) -> (i32, i32, i32, i32) {
    let x = (rect.min.x * pixels_per_point).round() as i32;
    let y = (rect.min.y * pixels_per_point).round() as i32;
    let right = (rect.max.x * pixels_per_point).round() as i32;
    let bottom = (rect.max.y * pixels_per_point).round() as i32;
    let x = x.clamp(0, width);
    let y = y.clamp(0, height);
    let right = right.clamp(x, width);
    let bottom = bottom.clamp(y, height);
    (x, y, right - x, bottom - y)
}

struct GuiArgs {
    control: Option<u16>,
    bridge: Option<u16>,
    file: Option<PathBuf>,
}

fn parse_gui(args: &[String]) -> Result<GuiArgs, String> {
    let mut control = std::env::var("FROXEL_CONTROL_PORT")
        .ok()
        .map(|text| parse_port(&text))
        .transpose()?;
    let mut bridge = std::env::var("FROXEL_BRIDGE_PORT")
        .ok()
        .map(|text| parse_port(&text))
        .transpose()?;
    let mut file = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                eprintln!(
                    "\
usage: froxel [--control PORT] [--bridge PORT] [file.vox]
       froxel bridge --input <file.vox> [--port PORT]
       froxel mcp [--connect HOST:PORT] [--launch]
       froxel render --input <file.vox> --output <file.png>
       froxel voxrt --input <file.vox> [--dump]"
                );
                std::process::exit(0);
            }
            "--control" => {
                i += 1;
                let text = args
                    .get(i)
                    .filter(|s| !s.starts_with('-'))
                    .ok_or_else(|| "--control needs a port".to_string())?;
                control = Some(parse_port(text)?);
            }
            "--bridge" => {
                i += 1;
                let text = args
                    .get(i)
                    .filter(|s| !s.starts_with('-'))
                    .ok_or_else(|| "--bridge needs a port".to_string())?;
                bridge = Some(parse_port(text)?);
            }
            other if other.starts_with('-') => return Err(format!("unknown argument `{other}`")),
            other => {
                if file.is_some() {
                    return Err(format!("unexpected argument `{other}`"));
                }
                file = Some(PathBuf::from(other));
            }
        }
        i += 1;
    }
    Ok(GuiArgs {
        control,
        bridge,
        file,
    })
}

fn parse_port(text: &str) -> Result<u16, String> {
    text.parse::<u16>()
        .map_err(|_| format!("port `{text}` is not a number"))
}

/// Load a system UI sans close to Godot's editor font (Inter), and the
/// vendored Phosphor subset used by the tool icons.
///
/// San Francisco is the macOS UI face; the others are regular single-face
/// fallbacks. Collections are skipped so a hairline weight is not picked by
/// index. Returns false when no UI face is present, in which case the theme
/// uses a larger default size. The icon font is installed either way.
fn install_ui_font(ctx: &egui::Context) -> bool {
    let mut fonts = egui::FontDefinitions::default();
    let loaded = load_system_ui(&mut fonts);
    fonts.font_data.insert(
        "Phosphor".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/icons/Phosphor.ttf"
        ))),
    );
    fonts.families.insert(
        egui::FontFamily::Name("Phosphor".into()),
        vec!["Phosphor".to_owned()],
    );
    ctx.set_fonts(fonts);
    loaded
}

fn load_system_ui(fonts: &mut egui::FontDefinitions) -> bool {
    const CANDIDATES: &[&str] = &[
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/System/Library/Fonts/Geneva.ttf",
        "/System/Library/Fonts/Supplemental/Verdana.ttf",
    ];
    for path in CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        // TrueType outlines. egui panics on a face it cannot parse.
        if bytes.len() < 12 || bytes[0..4] != [0, 1, 0, 0] {
            continue;
        }
        fonts.font_data.insert(
            "UI".to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.insert(0, "UI".to_owned());
        }
        return true;
    }
    false
}

fn apply_theme(ctx: &egui::Context) {
    // egui follows the system appearance by default and the visuals below
    // only apply to the active theme, so light mode showed stock light panels.
    ctx.set_theme(egui::Theme::Dark);
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));
    let font_loaded = install_ui_font(ctx);

    // Godot 4 default dark: blue-gray base, accent ~#4591fb.
    let panel = Color32::from_rgb(44, 51, 62);
    let window = Color32::from_rgb(36, 42, 52);
    let recessed = Color32::from_rgb(26, 31, 40);
    let faint = Color32::from_rgb(54, 62, 76);
    let button = Color32::from_rgb(62, 71, 86);
    let button_hover = Color32::from_rgb(80, 92, 110);
    let border = Color32::from_rgb(118, 132, 154);
    let border_soft = Color32::from_rgb(86, 98, 116);
    let text = Color32::from_rgb(230, 234, 242);
    let text_strong = Color32::from_rgb(246, 248, 252);
    let accent = Color32::from_rgb(69, 145, 251);

    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(text);
    visuals.window_fill = window;
    visuals.panel_fill = panel;
    visuals.extreme_bg_color = recessed;
    visuals.faint_bg_color = faint;
    visuals.code_bg_color = recessed;
    visuals.hyperlink_color = Color32::from_rgb(148, 196, 255);
    visuals.window_corner_radius = egui::CornerRadius::ZERO;
    visuals.window_stroke = egui::Stroke::new(1.0_f32, border_soft);
    visuals.menu_corner_radius = egui::CornerRadius::same(3);
    visuals.slider_trailing_fill = true;

    let radius = egui::CornerRadius::same(3);
    visuals.widgets.noninteractive.bg_fill = panel;
    visuals.widgets.noninteractive.weak_bg_fill = panel;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, border_soft);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, text);
    visuals.widgets.noninteractive.corner_radius = radius;

    visuals.widgets.inactive.bg_fill = button;
    visuals.widgets.inactive.weak_bg_fill = button;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, border);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, text_strong);
    visuals.widgets.inactive.corner_radius = radius;

    visuals.widgets.hovered.bg_fill = button_hover;
    visuals.widgets.hovered.weak_bg_fill = button_hover;
    visuals.widgets.hovered.bg_stroke =
        egui::Stroke::new(1.0_f32, Color32::from_rgb(168, 182, 204));
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.hovered.corner_radius = radius;
    visuals.widgets.hovered.expansion = 0.0;

    visuals.widgets.active.bg_fill = accent;
    visuals.widgets.active.weak_bg_fill = accent;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, Color32::from_rgb(140, 190, 255));
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.active.corner_radius = radius;
    visuals.widgets.active.expansion = 0.0;

    visuals.widgets.open.bg_fill = Color32::from_rgb(48, 56, 70);
    visuals.widgets.open.weak_bg_fill = button;
    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0_f32, border);
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0_f32, text_strong);
    visuals.widgets.open.corner_radius = radius;

    visuals.selection.bg_fill = accent;
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, Color32::WHITE);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 5.0);
    style.spacing.button_padding = egui::vec2(8.0, 3.0);
    // Godot's editor default is 14. Without a UI face, go larger so the
    // built-in font stays readable.
    let (body, small, mono) = if font_loaded {
        (14.0, 12.0, 13.0)
    } else {
        (16.0, 13.0, 14.0)
    };
    for (kind, size) in [
        (egui::TextStyle::Body, body),
        (egui::TextStyle::Button, body),
        (egui::TextStyle::Heading, body + 2.0),
        (egui::TextStyle::Small, small),
        (egui::TextStyle::Monospace, mono),
    ] {
        if let Some(font) = style.text_styles.get_mut(&kind) {
            font.size = size;
        }
    }
    ctx.set_style_of(egui::Theme::Dark, style);
}
