use crate::custom_event::{OpenType, RuffleEvent};
use crate::gui::dialogs::Dialogs;
use crate::gui::{DebugMessage, text};
use crate::player::LaunchOptions;
use crate::preferences::GlobalPreferences;
use egui::{Button, Key, KeyboardShortcut, Modifiers, Widget};
use ruffle_core::config::Letterbox;
use ruffle_core::focus_tracker::DisplayObject;
use ruffle_core::{Player, StageScaleMode};
use ruffle_frontend_utils::content::ContentDescriptor;
use ruffle_frontend_utils::recents::Recent;
use ruffle_render::quality::StageQuality;
use unic_langid::LanguageIdentifier;
use winit::event_loop::EventLoopProxy;

pub struct MenuBar {
    event_loop: EventLoopProxy<RuffleEvent>,
    default_launch_options: LaunchOptions,
    preferences: GlobalPreferences,

    cached_recents: Option<Vec<Recent>>,
    seek_frame: Option<u16>,
    seek_was_playing: bool,
    pub currently_opened: Option<(ContentDescriptor, LaunchOptions)>,
}

impl MenuBar {
    const SHORTCUT_FULLSCREEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F11);
    const SHORTCUT_FULLSCREEN_WINDOWS: KeyboardShortcut =
        KeyboardShortcut::new(Modifiers::ALT, Key::Enter);
    const SHORTCUT_OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
    const SHORTCUT_OPEN_ADVANCED: KeyboardShortcut =
        KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::O);
    const SHORTCUT_PAUSE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::P);
    const SHORTCUT_STEP: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Space);
    const SHORTCUT_QUIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);

    pub fn new(
        event_loop: EventLoopProxy<RuffleEvent>,
        default_launch_options: LaunchOptions,
        preferences: GlobalPreferences,
    ) -> Self {
        Self {
            event_loop,
            default_launch_options,
            cached_recents: None,
            seek_frame: None,
            seek_was_playing: false,
            currently_opened: None,
            preferences,
        }
    }

    pub fn consume_shortcuts(
        &self,
        egui_ctx: &egui::Context,
        dialogs: &mut Dialogs,
        mut player: Option<&mut Player>,
    ) {
        // TODO(mike): Make some MenuItem struct with shortcut info to handle this more cleanly.
        if egui_ctx.input_mut(|input| input.consume_shortcut(&Self::SHORTCUT_OPEN_ADVANCED)) {
            dialogs.open_file_advanced();
        }
        if egui_ctx.input_mut(|input| input.consume_shortcut(&Self::SHORTCUT_OPEN)) {
            self.browse_and_open(OpenType::File);
        }
        if egui_ctx.input_mut(|input| input.consume_shortcut(&Self::SHORTCUT_QUIT)) {
            self.request_exit();
        }

        if let Some(player) = &mut player {
            let playing = player.is_playing();
            if egui_ctx.input_mut(|input| input.consume_shortcut(&Self::SHORTCUT_PAUSE)) {
                player.set_is_playing(!playing);
            }
            if !playing && egui_ctx.input_mut(|input| input.consume_shortcut(&Self::SHORTCUT_STEP))
            {
                player.suspend_after_next_frame();
            }
        }

        let mut fullscreen_pressed =
            egui_ctx.input_mut(|input| input.consume_shortcut(&Self::SHORTCUT_FULLSCREEN));
        if cfg!(windows) && !fullscreen_pressed {
            // TODO We can remove this shortcut when we add some kind of preferences.
            fullscreen_pressed = egui_ctx
                .input_mut(|input| input.consume_shortcut(&Self::SHORTCUT_FULLSCREEN_WINDOWS));
        }
        if let Some(player) = &mut player
            && fullscreen_pressed
        {
            let is_fullscreen = player.is_fullscreen();
            player.set_fullscreen(!is_fullscreen);
        }
    }

    pub fn show(
        &mut self,
        locale: &LanguageIdentifier,
        egui_ui: &mut egui::Ui,
        dialogs: &mut Dialogs,
        mut player: Option<&mut Player>,
    ) {
        egui::Panel::top("menu_bar").exact_size(crate::gui::MENU_HEIGHT as f32).show(egui_ui, |ui| {
             egui::MenuBar::new().ui(ui, |ui| {
                self.file_menu(locale, ui, dialogs, player.is_some());
                self.view_menu(locale, ui, &mut player);
                self.controls_menu(locale, ui, dialogs, &mut player);
                ui.separator();
                if ui.button("開啟 SWF...").clicked() {
                    self.browse_and_open(OpenType::File);
                }
                if let Some(player) = &mut player {
                    let mut rate = player.playback_rate();
                    egui::ComboBox::from_id_salt("playback_speed")
                        .selected_text(format!("倍速：{rate}x"))
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for option in [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0] {
                                ui.selectable_value(&mut rate, option, format!("{option}x"));
                            }
                        });
                    if rate != player.playback_rate() {
                        player.set_playback_rate(rate);
                    }
                }
                ui.menu_button( text(locale, "bookmarks-menu"), |ui| {
                    if Button::new(text(locale, "bookmarks-menu-add")).ui(ui).clicked() {
                        ui.close();

                        let content_descriptor = self.currently_opened.as_ref().map(|(desc, _)| desc.clone());
                        dialogs.open_add_bookmark(content_descriptor);
                    }

                    if Button::new(text(locale, "bookmarks-menu-manage")).ui(ui).clicked() {
                        ui.close();
                        dialogs.open_bookmarks();
                    }

                    if self.preferences.have_bookmarks() {
                        ui.separator();
                        self.preferences.bookmarks(|bookmarks| {
                            for bookmark in bookmarks.iter().filter(|x| !x.is_invalid()) {
                                if Button::new(&bookmark.name).ui(ui).clicked() {
                                    ui.close();
                                    let _ = self.event_loop.send_event(RuffleEvent::Open(
                                        bookmark.content_descriptor.clone(),
                                        Box::new(self.default_launch_options.clone()),
                                    ));
                                }
                            }
                        });
                    }
                });
                ui.menu_button(text(locale, "debug-menu"), |ui| {
                    ui.add_enabled_ui(player.is_some(), |ui| {
                        if Button::new(text(locale, "debug-menu-open-stage")).ui(ui).clicked() {
                            ui.close();
                            if let Some(player) = &mut player {
                                player.debug_ui().queue_message(DebugMessage::TrackStage);
                            }
                        }
                        if let Some(player) = &mut player {
                            let mut has_root_movie_clip = false;
                            player.mutate_with_update_context(|ctx| {
                                has_root_movie_clip = matches!(ctx.stage.root_clip(), Some(DisplayObject::MovieClip(_)));
                            });
                            let button = Button::new(text(locale, "debug-menu-open-root-movie-clip"));
                            if ui.add_enabled(has_root_movie_clip, button).clicked() {
                                ui.close();
                                player.debug_ui().queue_message(DebugMessage::TrackRootMovieClip);
                            }
                        }
                        ui.separator();
                        if Button::new(text(locale, "debug-menu-open-movie")).ui(ui).clicked() {
                            ui.close();
                            if let Some(player) = &mut player {
                                player.debug_ui().queue_message(DebugMessage::TrackTopLevelMovie);
                            }
                        }
                        if Button::new(text(locale, "debug-menu-open-movie-list")).ui(ui).clicked() {
                            ui.close();
                            if let Some(player) = &mut player {
                                player.debug_ui().queue_message(DebugMessage::ShowKnownMovies);
                            }
                        }
                        if Button::new(text(locale, "debug-menu-open-domain-list")).ui(ui).clicked() {
                            ui.close();
                            if let Some(player) = &mut player {
                                player.debug_ui().queue_message(DebugMessage::ShowDomains);
                            }
                        }
                        ui.separator();
                        if Button::new(text(locale, "debug-menu-search-display-objects")).ui(ui).clicked() {
                            ui.close();
                            if let Some(player) = &mut player {
                                player.debug_ui().queue_message(DebugMessage::SearchForDisplayObject);
                            }
                        }
                    });
                });
                ui.menu_button(text(locale, "help-menu"), |ui| {
                    if ui.button(text(locale, "help-menu-join-discord")).clicked() {
                        self.launch_website(ui, "https://discord.gg/ruffle");
                    }
                    if ui.button(text(locale, "help-menu-report-a-bug")).clicked() {
                        self.launch_website(ui, "https://github.com/ruffle-rs/ruffle/issues/new?assignees=&labels=bug&projects=&template=bug_report.yml");
                    }
                    if ui.button(text(locale, "help-menu-sponsor-development")).clicked() {
                        self.launch_website(ui, "https://opencollective.com/ruffle/");
                    }
                    if ui.button(text(locale, "help-menu-translate-ruffle")).clicked() {
                        self.launch_website(ui, "https://crowdin.com/project/ruffle");
                    }
                    ui.separator();
                    if ui.button(text(locale, "help-menu-about")).clicked() {
                        dialogs.open_about_screen();
                        ui.close();
                    }
                });
            });
        });
        egui::Panel::bottom("playback_controls")
            .exact_size(crate::gui::CONTROLS_HEIGHT as f32)
            .show(egui_ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(player) = &mut player {
                    let playing = player.is_playing();
                    let label = if playing { "暫停 (Ctrl+P)" } else { "播放 (Ctrl+P)" };
                    let response = ui.add(Button::new("").min_size(egui::vec2(44.0, 32.0)))
                        .on_hover_text(label);
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
                    let center = response.rect.center();
                    let color = ui.visuals().widgets.inactive.fg_stroke.color;
                    if playing {
                        for offset in [-5.0, 5.0] {
                            ui.painter().rect_filled(egui::Rect::from_center_size(
                                center + egui::vec2(offset, 0.0), egui::vec2(4.0, 16.0)), 0.0, color);
                        }
                    } else {
                        ui.painter().add(egui::Shape::convex_polygon(vec![
                            center + egui::vec2(-6.0, -9.0),
                            center + egui::vec2(-6.0, 9.0),
                            center + egui::vec2(9.0, 0.0),
                        ], color, egui::Stroke::NONE));
                    }
                    if response.clicked() {
                        player.set_is_playing(!playing);
                    }
                    if ui.add(Button::new("截圖").min_size(egui::vec2(52.0, 32.0)))
                        .on_hover_text("擷取當前影片畫面並另存為 PNG（不含控制列）")
                        .clicked()
                    {
                        self.save_screenshot(player, dialogs);
                    }
                }
                if let Some(player) = &mut player
                    && let Some((current, total, fps)) = player.timeline_position()
                    && total > 1 && fps.is_finite() && fps > 0.0
                {
                    ui.label("進度");
                    let mut frame = self.seek_frame.unwrap_or(current.max(1));
                    ui.spacing_mut().slider_width = (ui.available_width() - 125.0).max(80.0);
                    let response = ui.add(egui::Slider::new(&mut frame, 1..=total).show_value(false))
                        .on_hover_text("拖曳或點擊跳轉；放開後影音同步從新位置播放");
                    if response.drag_started() {
                        self.seek_was_playing = player.is_playing();
                        player.set_is_playing(false);
                    }
                    if response.dragged() {
                        self.seek_frame = Some(frame);
                    }
                    if response.drag_stopped() {
                        player.seek_to_frame(frame);
                        player.set_is_playing(self.seek_was_playing);
                        self.seek_frame = None;
                    } else if response.changed() && !response.dragged() {
                        player.seek_to_frame(frame);
                    }
                    let time = |frame: u16| {
                        let seconds = (f64::from(frame.saturating_sub(1)) / fps) as u64;
                        format!("{:02}:{:02}", seconds / 60, seconds % 60)
                    };
                    ui.label(format!("{} / {}", time(frame), time(total)));
                } else {
                    self.seek_frame = None;
                    ui.weak("開啟 SWF 後可拖曳進度條");
                }
            });
        });
    }

    fn save_screenshot(&self, player: &mut Player, dialogs: &Dialogs) {
        use crate::gui::dialogs::message_dialog::MessageDialogConfiguration;
        use crate::gui::{DialogDescriptor, LocalizableText, MovieView};
        use ruffle_render_wgpu::backend::WgpuRenderBackend;
        use std::any::Any;

        // Capture before opening the save dialog: the saved image is the frame
        // visible when clicked, even if the movie continues playing afterwards.
        let result = <dyn Any>::downcast_ref::<WgpuRenderBackend<MovieView>>(player.renderer_mut())
            .ok_or_else(|| anyhow::anyhow!("無法取得影片畫面"))
            .and_then(|renderer| renderer.target().capture_png_frame(renderer.descriptors()));
        let report = {
            let event_loop = self.event_loop.clone();
            move |body: String| {
                let _ = event_loop.send_event(RuffleEvent::OpenDialog(DialogDescriptor::ShowMessage(
                    MessageDialogConfiguration::new(
                        LocalizableText::NonLocalizedText("影片截圖".into()),
                        LocalizableText::NonLocalizedText(body.into()),
                    ),
                )));
            }
        };
        let image = match result {
            Ok(image) => image,
            Err(error) => { report(format!("截圖失敗：{error}")); return; }
        };
        let filename = format!("SWF-截圖-{}.png", chrono::Local::now().format("%Y%m%d-%H%M%S-%3f"));
        let dialog = rfd::AsyncFileDialog::new()
            .set_title("儲存影片截圖")
            .add_filter("PNG 圖片", &["png"])
            .set_file_name(filename);
        if let Some(pick) = dialogs.file_picker().show_dialog(dialog, |dialog| dialog.save_file()) {
            tokio::spawn(async move {
                let Some(file) = pick.await else { return; };
                let path = file.path().to_owned();
                let display_path = path.display().to_string();
                match tokio::task::spawn_blocking(move || image.save_with_format(path, image::ImageFormat::Png)).await {
                    Ok(Ok(())) => report(format!("截圖已儲存：\n{display_path}")),
                    Ok(Err(error)) => report(format!("儲存失敗：{error}")),
                    Err(error) => report(format!("儲存失敗：{error}")),
                }
            });
        }
    }

    fn file_menu(
        &mut self,
        locale: &LanguageIdentifier,
        ui: &mut egui::Ui,
        dialogs: &mut Dialogs,
        player_exists: bool,
    ) {
        ui.menu_button(text(locale, "file-menu"), |ui| {
            if Button::new(text(locale, "file-menu-open-file"))
                .shortcut_text(ui.ctx().format_shortcut(&Self::SHORTCUT_OPEN))
                .ui(ui)
                .clicked()
            {
                ui.close();
                self.browse_and_open(OpenType::File);
            }

            if Button::new(text(locale, "file-menu-open-directory"))
                .ui(ui)
                .clicked()
            {
                ui.close();
                self.browse_and_open(OpenType::Directory);
            }

            if Button::new(text(locale, "file-menu-open-advanced"))
                .shortcut_text(ui.ctx().format_shortcut(&Self::SHORTCUT_OPEN_ADVANCED))
                .ui(ui)
                .clicked()
            {
                ui.close();
                dialogs.open_file_advanced();
            }
            ui.separator();

            if ui
                .add_enabled(player_exists, Button::new(text(locale, "file-menu-reload")))
                .clicked()
            {
                self.reload_movie(ui);
            }

            if ui
                .add_enabled(player_exists, Button::new(text(locale, "file-menu-close")))
                .clicked()
            {
                self.close_movie(ui);
            }
            ui.separator();

            let recent_menu_response = ui
                .menu_button(text(locale, "file-menu-recents"), |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    ui.set_min_width(250.0);

                    if self
                        .cached_recents
                        .as_ref()
                        .map(|x| x.is_empty())
                        .unwrap_or(true)
                    {
                        ui.label(text(locale, "file-menu-recents-empty"));
                    }

                    if let Some(recents) = &self.cached_recents {
                        for recent in recents {
                            if ui.button(&recent.name).clicked() {
                                ui.close();
                                let _ = self.event_loop.send_event(RuffleEvent::Open(
                                    recent.content_descriptor.clone(),
                                    Box::new(self.default_launch_options.clone()),
                                ));
                            }
                        }
                    };
                })
                .inner;

            match recent_menu_response {
                // recreate the cache on the first draw.
                Some(_) if self.cached_recents.is_none() => {
                    self.cached_recents = Some(self.preferences.recents(|recents| {
                        recents
                            .iter()
                            .rev()
                            .filter(|x| !x.is_invalid() && x.is_available())
                            .cloned()
                            .collect::<Vec<_>>()
                    }))
                }
                // clear cache, since menu was closed.
                None if self.cached_recents.is_some() => self.cached_recents = None,
                _ => {}
            }
            ui.separator();

            if ui
                .add_enabled(player_exists, Button::new(text(locale, "file-menu-export")))
                .clicked()
            {
                self.export_bundle(ui);
            }
            ui.separator();

            if Button::new(text(locale, "file-menu-preferences"))
                .ui(ui)
                .clicked()
            {
                ui.close();
                dialogs.open_preferences();
            }
            ui.separator();

            if Button::new(text(locale, "file-menu-exit"))
                .shortcut_text(ui.ctx().format_shortcut(&Self::SHORTCUT_QUIT))
                .ui(ui)
                .clicked()
            {
                ui.close();
                self.request_exit();
            }
        });
    }

    fn view_menu(
        &self,
        locale: &LanguageIdentifier,
        ui: &mut egui::Ui,
        player: &mut Option<&mut Player>,
    ) {
        ui.menu_button(text(locale, "view-menu"), |ui| {
            ui.add_enabled_ui(player.is_some(), |ui| {
                ui.menu_button(text(locale, "scale-mode"), |ui| {
                    let items = [
                        (
                            "scale-mode-noscale",
                            "scale-mode-noscale-tooltip",
                            StageScaleMode::NoScale,
                        ),
                        (
                            "scale-mode-showall",
                            "scale-mode-showall-tooltip",
                            StageScaleMode::ShowAll,
                        ),
                        (
                            "scale-mode-exactfit",
                            "scale-mode-exactfit-tooltip",
                            StageScaleMode::ExactFit,
                        ),
                        (
                            "scale-mode-noborder",
                            "scale-mode-noborder-tooltip",
                            StageScaleMode::NoBorder,
                        ),
                    ];
                    let current_scale_mode = player.as_mut().map(|player| player.scale_mode());
                    for (id, tooltip_id, scale_mode) in items {
                        let response = if Some(scale_mode) == current_scale_mode {
                            ui.checkbox(&mut true, text(locale, id))
                        } else {
                            ui.button(text(locale, id))
                        }
                        .on_hover_text_at_pointer(text(locale, tooltip_id));
                        if response.clicked() {
                            ui.close();
                            if let Some(player) = player {
                                player.set_scale_mode(scale_mode);
                            }
                        }
                    }
                    ui.separator();

                    let original_forced_scale_mode = player
                        .as_mut()
                        .map(|player| player.forced_scale_mode())
                        .unwrap_or_default();
                    let mut forced_scale_mode = original_forced_scale_mode;
                    ui.checkbox(&mut forced_scale_mode, text(locale, "scale-mode-force"))
                        .on_hover_text_at_pointer(text(locale, "scale-mode-force-tooltip"));
                    if let Some(player) = player
                        && forced_scale_mode != original_forced_scale_mode
                    {
                        player.set_forced_scale_mode(forced_scale_mode);
                    }
                });

                let original_letterbox = if let Some(player) = player {
                    player.letterbox() == Letterbox::On
                } else {
                    false
                };
                let mut letterbox = original_letterbox;
                ui.checkbox(&mut letterbox, text(locale, "letterbox"));
                if let Some(player) = player
                    && letterbox != original_letterbox
                {
                    player.set_letterbox(if letterbox {
                        Letterbox::On
                    } else {
                        Letterbox::Off
                    });
                }
                ui.separator();

                if Button::new(text(locale, "view-menu-fullscreen"))
                    .shortcut_text(ui.ctx().format_shortcut(&Self::SHORTCUT_FULLSCREEN))
                    .ui(ui)
                    .clicked()
                {
                    ui.close();
                    if let Some(player) = player {
                        player.set_fullscreen(true);
                    }
                }
                ui.separator();

                ui.menu_button(text(locale, "quality"), |ui| {
                    let items = [
                        ("quality-low", StageQuality::Low),
                        ("quality-medium", StageQuality::Medium),
                        ("quality-high", StageQuality::High),
                        ("quality-best", StageQuality::Best),
                        ("quality-high8x8", StageQuality::High8x8),
                        ("quality-high8x8linear", StageQuality::High8x8Linear),
                        ("quality-high16x16", StageQuality::High16x16),
                        ("quality-high16x16linear", StageQuality::High16x16Linear),
                    ];
                    let current_quality = player.as_mut().map(|player| player.quality());
                    for (id, quality) in items {
                        let clicked = if Some(quality) == current_quality {
                            ui.checkbox(&mut true, text(locale, id)).clicked()
                        } else {
                            ui.button(text(locale, id)).clicked()
                        };
                        if clicked {
                            ui.close();
                            if let Some(player) = player {
                                player.set_quality(quality);
                            }
                        }
                    }
                });
            });
        });
    }

    fn controls_menu(
        &self,
        locale: &LanguageIdentifier,
        ui: &mut egui::Ui,
        dialogs: &mut Dialogs,
        player: &mut Option<&mut Player>,
    ) {
        ui.menu_button(text(locale, "controls-menu"), |ui| {
            ui.add_enabled_ui(player.is_some(), |ui| {
                let playing = player.as_ref().map(|p| p.is_playing()).unwrap_or_default();
                let btn_name = if playing {
                    "controls-menu-suspend"
                } else {
                    "controls-menu-resume"
                };
                if Button::new(text(locale, btn_name))
                    .shortcut_text(ui.ctx().format_shortcut(&Self::SHORTCUT_PAUSE))
                    .ui(ui)
                    .clicked()
                {
                    ui.close();
                    if let Some(player) = player {
                        player.set_is_playing(!playing);
                    }
                }

                ui.add_enabled_ui(!playing, |ui| {
                    if Button::new(text(locale, "controls-menu-step-once"))
                        .shortcut_text(ui.ctx().format_shortcut(&Self::SHORTCUT_STEP))
                        .ui(ui)
                        .clicked()
                    {
                        ui.close();
                        if let Some(player) = player {
                            player.suspend_after_next_frame();
                        }
                    }
                });
            });
            if Button::new(text(locale, "controls-menu-volume"))
                .ui(ui)
                .clicked()
            {
                dialogs.open_volume_controls();
                ui.close();
            }
        });
    }

    fn browse_and_open(&self, open_type: OpenType) {
        let _ = self.event_loop.send_event(RuffleEvent::BrowseAndOpen(
            Box::new(self.default_launch_options.clone()),
            open_type,
        ));
    }

    fn close_movie(&mut self, ui: &egui::Ui) {
        let _ = self.event_loop.send_event(RuffleEvent::CloseFile);
        self.currently_opened = None;
        ui.close();
    }

    fn reload_movie(&mut self, ui: &egui::Ui) {
        let _ = self.event_loop.send_event(RuffleEvent::CloseFile);
        if let Some((movie_url, opts)) = self.currently_opened.take() {
            let _ = self
                .event_loop
                .send_event(RuffleEvent::Open(movie_url, opts.into()));
        }
        ui.close();
    }

    fn request_exit(&self) {
        let _ = self.event_loop.send_event(RuffleEvent::ExitRequested);
    }

    fn launch_website(&self, ui: &egui::Ui, url: &str) {
        let _ = webbrowser::open(url);
        ui.close();
    }

    fn export_bundle(&self, ui: &egui::Ui) {
        let _ = self.event_loop.send_event(RuffleEvent::ExportBundle);
        ui.close();
    }
}
