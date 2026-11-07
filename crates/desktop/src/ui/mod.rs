mod archive_view;
mod icon_decode;
mod identity_editor;
mod install_smoke;
mod installation_view;
mod manager;
mod settings;
mod styles;

use adw::prelude::*;
use gtk::{gio, glib};
use std::{
    cell::RefCell,
    collections::VecDeque,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};
use unpack_core::{
    archive::{self, Inspection},
    config::{OpenBehavior, Paths, Settings, Theme},
    detection::{self, Detection},
    installer::{self, Manifest, Plan},
    Cancellation, Progress,
};

pub(super) enum Event {
    Progress(Progress),
    Inspected(Inspection, Detection),
    Installed(Manifest),
    Registry(Vec<Manifest>, Vec<installer::HistoryEvent>),
    BackgroundWarning(String),
    Recovered(Vec<String>),
    Done(String),
    Failure(String),
    ArchiveIcon {
        archive_hash: String,
        path: PathBuf,
        png: std::result::Result<icon_decode::Pixels, String>,
    },
    MetadataUpdated(Manifest),
}
pub(super) struct State {
    pub settings: Settings,
    pub inspection: Option<Inspection>,
    pub detection: Option<Detection>,
    pub busy: bool,
    pub cancel: Cancellation,
    pub queue: VecDeque<(PathBuf, String)>,
    pub applications: Vec<Manifest>,
    pub choosing_open: bool,
    pub settings_writable: bool,
    pub prefer_install: bool,
}

pub struct Window {
    pub window: adw::ApplicationWindow,
    pub(super) paths: Paths,
    pub(super) state: RefCell<State>,
    pub(super) sender: async_channel::Sender<Event>,
    pub(super) stack: gtk::Stack,
    pub(super) toast: adw::ToastOverlay,
    pub(super) navigation: adw::OverlaySplitView,
    pub(super) status: gtk::Label,
    pub(super) progress: gtk::ProgressBar,
    pub(super) cancel_button: gtk::Button,
    archive: archive_view::ArchiveView,
    pub(super) apps_list: gtk::ListBox,
    pub(super) history_list: gtk::ListBox,
}

pub(super) fn margins(widget: &impl IsA<gtk::Widget>, size: i32) {
    widget.set_margin_top(size);
    widget.set_margin_bottom(size);
    widget.set_margin_start(size);
    widget.set_margin_end(size);
}
pub(super) fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.add_css_class(class);
    label
}
pub(super) fn row_box() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 12);
    margins(&b, 24);
    b
}
pub(super) fn scroll(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(child)
        .build()
}

pub(super) fn host_launch_context() -> gio::AppLaunchContext {
    let context = gio::AppLaunchContext::new();
    for (key, value) in unpack_core::desktop::child_environment() {
        if let Some(value) = value {
            context.setenv(key, value);
        } else {
            context.unsetenv(key);
        }
    }
    context
}

impl Window {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        styles::install();
        let (paths, initial_error) = match Paths::discover() {
            Ok(paths) => (paths, None),
            Err(error) => {
                eprintln!("Cannot determine XDG directories: {error}");
                std::process::exit(1);
            }
        };
        let (settings, initial_error) = match Settings::load(&paths) {
            Ok(settings) => (settings, initial_error),
            Err(error) => (Settings::default(), Some(format!("Your settings could not be loaded: {error}. Defaults are active; the original file has not been overwritten."))),
        };
        apply_theme(settings.theme);
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Unpack")
            .default_width(if settings.restore_window {
                settings.window_width
            } else {
                1100
            })
            .default_height(if settings.restore_window {
                settings.window_height
            } else {
                760
            })
            .width_request(600)
            .height_request(420)
            .build();
        if settings.restore_window && settings.maximized {
            window.maximize();
        }
        let stack = gtk::Stack::builder()
            .hhomogeneous(false)
            .vhomogeneous(false)
            .transition_type(gtk::StackTransitionType::Crossfade)
            .hexpand(true)
            .vexpand(true)
            .build();
        let home = adw::StatusPage::builder()
            .icon_name("package-x-generic-symbolic")
            .title("A better home for your downloads")
            .description(
                "Explore archives. Install portable applications. Keep everything in order.",
            )
            .build();
        let home_buttons = gtk::Box::new(gtk::Orientation::Vertical, 12);
        home_buttons.set_halign(gtk::Align::Center);
        let open = gtk::Button::with_label("Open an Archive…");
        open.add_css_class("suggested-action");
        open.add_css_class("pill");
        open.set_action_name(Some("win.open"));
        let create = gtk::Button::with_label("Create an Archive…");
        create.set_action_name(Some("win.create"));
        home_buttons.append(&open);
        home_buttons.append(&create);
        home_buttons.append(&label("Drop an archive anywhere to get started.\nYour files stay yours. No administrator access needed.", "dim-label"));
        home.set_child(Some(&home_buttons));
        stack.add_named(&home, Some("home"));
        let archive = archive_view::ArchiveView::new();
        stack.add_named(&archive.widget, Some("archive"));
        let apps_list = gtk::ListBox::new();
        apps_list.set_selection_mode(gtk::SelectionMode::None);
        apps_list.add_css_class("boxed-list");
        let apps_page = row_box();
        apps_page.append(&label("Installed Applications", "title-1"));
        apps_page.append(&label(
            "Portable applications managed by Unpack. Your system packages are not changed.",
            "dim-label",
        ));
        apps_page.append(&apps_list);
        stack.add_named(&scroll(&apps_page), Some("applications"));
        let history_list = gtk::ListBox::new();
        history_list.set_selection_mode(gtk::SelectionMode::None);
        history_list.add_css_class("boxed-list");
        let history_page = row_box();
        history_page.append(&label("Installation History", "title-1"));
        history_page.append(&label(
            "A transparent record of the changes Unpack has made.",
            "dim-label",
        ));
        history_page.append(&history_list);
        stack.add_named(&scroll(&history_page), Some("history"));

        let header = adw::HeaderBar::new();
        let toggle = gtk::ToggleButton::builder()
            .icon_name("sidebar-show-symbolic")
            .tooltip_text("Show navigation")
            .build();
        header.pack_start(&toggle);
        let open = gtk::Button::builder()
            .icon_name("document-open-symbolic")
            .tooltip_text("Open archive (Ctrl+O)")
            .action_name("win.open")
            .build();
        header.pack_start(&open);
        let title = adw::WindowTitle::new("Unpack", "Archives, made approachable");
        header.set_title_widget(Some(&title));
        let menu = gio::Menu::new();
        menu.append(Some("Create Archive…"), Some("win.create"));
        menu.append(Some("Preferences"), Some("win.settings"));
        menu.append(Some("Help"), Some("win.help"));
        menu.append(Some("About Unpack"), Some("win.about"));
        menu.append(Some("Quit"), Some("app.quit"));
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .build();
        header.pack_end(&menu_button);
        let content = adw::ToolbarView::new();
        content.add_top_bar(&header);
        content.set_content(Some(&stack));
        let status_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        margins(&status_box, 8);
        let progress = gtk::ProgressBar::new();
        progress.set_visible(false);
        let status_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let status = label("Ready", "dim-label");
        status.set_hexpand(true);
        status.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        status.set_wrap(false);
        let cancel_button = gtk::Button::with_label("Cancel");
        cancel_button.set_action_name(Some("win.cancel"));
        cancel_button.set_visible(false);
        status_row.append(&status);
        status_row.append(&cancel_button);
        status_box.append(&progress);
        status_box.append(&status_row);
        content.add_bottom_bar(&status_box);
        let navigation = adw::OverlaySplitView::builder()
            .content(&content)
            .min_sidebar_width(220.0)
            .max_sidebar_width(240.0)
            .build();
        let sidebar = gtk::ListBox::new();
        sidebar.set_selection_mode(gtk::SelectionMode::Single);
        sidebar.add_css_class("navigation-sidebar");
        margins(&sidebar, 8);
        for (name, icon) in [
            ("Home", "go-home-symbolic"),
            ("Archive", "package-x-generic-symbolic"),
            (
                "Installed Applications",
                "application-x-executable-symbolic",
            ),
            ("History", "document-open-recent-symbolic"),
        ] {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            margins(&row, 10);
            row.append(&gtk::Image::from_icon_name(icon));
            row.append(&label(name, "body"));
            sidebar.append(&row);
        }
        let side_toolbar = adw::ToolbarView::new();
        let side_header = adw::HeaderBar::new();
        side_header.set_title_widget(Some(&label("Library", "heading")));
        side_toolbar.add_top_bar(&side_header);
        side_toolbar.set_content(Some(&sidebar));
        navigation.set_sidebar(Some(&side_toolbar));
        navigation
            .bind_property("show-sidebar", &toggle, "active")
            .bidirectional()
            .sync_create()
            .build();
        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&navigation));
        window.set_content(Some(&toast));
        let (sender, receiver) = async_channel::unbounded();
        let settings_writable = initial_error.is_none();
        let ui = Rc::new(Self {
            window,
            paths,
            state: RefCell::new(State {
                settings,
                inspection: None,
                detection: None,
                busy: false,
                cancel: Cancellation::default(),
                queue: VecDeque::new(),
                applications: vec![],
                choosing_open: false,
                settings_writable,
                prefer_install: true,
            }),
            sender,
            stack,
            toast,
            navigation,
            status,
            progress,
            cancel_button,
            archive,
            apps_list,
            history_list,
        });
        let weak = Rc::downgrade(&ui);
        sidebar.connect_row_selected(move |_, row| {
            if let (Some(ui), Some(row)) = (weak.upgrade(), row) {
                let name = ["home", "archive", "applications", "history"][row.index() as usize];
                ui.stack.set_visible_child_name(name);
                if ui.navigation.is_collapsed() {
                    ui.navigation.set_show_sidebar(false);
                }
                if matches!(name, "applications" | "history") {
                    ui.refresh_registry();
                }
            }
        });
        let weak = Rc::downgrade(&ui);
        glib::spawn_future_local(async move {
            while let Ok(event) = receiver.recv().await {
                if let Some(ui) = weak.upgrade() {
                    ui.event(event);
                } else {
                    break;
                }
            }
        });
        ui.connect_actions();
        ui.connect_archive();
        ui.connect_close();
        let drop = gtk::DropTarget::new(gio::File::static_type(), gtk::gdk::DragAction::COPY);
        let weak = Rc::downgrade(&ui);
        drop.connect_drop(move |_, value, _, _| {
            if let (Some(ui), Ok(file)) = (weak.upgrade(), value.get::<gio::File>()) {
                if let Some(path) = file.path() {
                    ui.enqueue(path, String::new());
                    return true;
                }
            }
            false
        });
        ui.window.add_controller(drop);
        if let Some(error) = initial_error {
            let weak = Rc::downgrade(&ui);
            glib::idle_add_local_once(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.error(&error);
                }
            });
        }
        ui
    }

    fn connect_actions(self: &Rc<Self>) {
        for name in [
            "open", "create", "install", "primary", "extract", "search", "cancel", "settings",
            "help", "about",
        ] {
            let action = gio::SimpleAction::new(name, None);
            let weak = Rc::downgrade(self);
            action.connect_activate(move |action, _| {
                if let Some(ui) = weak.upgrade() {
                    match action.name().as_str() {
                        "open" => ui.open_dialog(),
                        "create" => ui.create_dialog(),
                        "install" => ui.review_install(),
                        "primary" => ui.primary_action(),
                        "extract" => ui.extract_dialog(),
                        "search" => {
                            ui.stack.set_visible_child_name("archive");
                            ui.archive.pages.set_visible_child_name("files");
                            ui.archive.search.grab_focus();
                        }
                        "cancel" => {
                            ui.state.borrow().cancel.cancel();
                        }
                        "settings" => ui.preferences(),
                        "help" => ui.help(),
                        "about" => ui.about(),
                        _ => {}
                    }
                }
            });
            self.window.add_action(&action);
        }
    }
    fn connect_close(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |window| {
            if let Some(ui) = weak.upgrade() {
                if ui.state.borrow().busy {
                    ui.state.borrow().cancel.cancel();
                    ui.error(
                        "An operation is being cancelled. Wait for cleanup before closing Unpack.",
                    );
                    return glib::Propagation::Stop;
                }
                let mut state = ui.state.borrow_mut();
                state.settings.maximized = window.is_maximized();
                if !window.is_maximized() {
                    state.settings.window_width = window.width().max(600);
                    state.settings.window_height = window.height().max(420);
                }
                if state.settings_writable {
                    if let Err(error) = state.settings.save(&ui.paths) {
                        eprintln!("Could not save window state: {error}");
                    }
                }
            }
            glib::Propagation::Proceed
        });
    }
    pub fn startup(self: &Rc<Self>) {
        if unpack_core::config::sandboxed_package()
            && std::env::var_os("UNPACK_SMOKE_TEST").is_none()
        {
            self.notify("Flatpak edition: archive tools are available. Host application installation requires the AppImage or native edition.");
        }
        let paths = self.paths.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || match installer::recover(&paths) {
            Ok(messages) => {
                let _ = sender.send_blocking(Event::Recovered(messages));
            }
            Err(e) => {
                let _ = sender.send_blocking(Event::BackgroundWarning(e.to_string()));
            }
        });
        if !self.state.borrow().settings.onboarding_complete {
            self.onboarding();
        }
        if std::env::var_os("UNPACK_SMOKE_TEST").is_some() {
            if std::env::var("FLATPAK_ID").is_ok_and(|id| id == "io.github.unpack.Unpack") {
                assert!(
                    unpack_core::config::sandboxed_package(),
                    "Installed Flatpak must activate host-installation restrictions"
                );
                eprintln!("Flatpak host-installation restriction active");
            }
            let weak = Rc::downgrade(self);
            glib::timeout_add_local_once(Duration::from_millis(600), move || {
                if let Some(ui) = weak.upgrade() {
                    assert!(
                        ui.state.borrow().inspection.is_some(),
                        "Smoke archive must load"
                    );
                    ui.window.set_default_size(640, 520);
                    ui.navigation.set_show_sidebar(false);
                    ui.stack.set_visible_child_name("archive");
                    ui.archive.pages.set_visible_child_name("files");
                    assert!(
                        !ui.archive.next.is_visible(),
                        "No queue button for a single archive"
                    );
                }
            });
            let weak = Rc::downgrade(self);
            glib::timeout_add_local_once(Duration::from_millis(1100), move || {
                if let Some(ui) = weak.upgrade() {
                    assert!(
                        ui.navigation.is_collapsed(),
                        "Main navigation must adapt at narrow widths"
                    );
                    ui.archive.pages.set_visible_child_name("install");
                    let state = ui.state.borrow();
                    let detected = state.detection.as_ref().expect("Archive detected");
                    assert_eq!(
                        detected.name, "Friendly Example",
                        "Name must come from the archive desktop entry"
                    );
                    assert!(detected.recommended_icon.is_some(), "Archive icon detected");
                    drop(state);
                    assert_eq!(ui.archive.app_name.text().as_str(), "Friendly Example");
                    assert!(ui.selected_archive_icon().is_some());
                    assert_eq!(
                        ui.archive.app_icon.storage_type(),
                        gtk::ImageType::Paintable,
                        "Archive icon preview loaded"
                    );
                    assert!(
                        ui.archive.review.is_visible(),
                        "Primary install action must remain visible"
                    );
                    assert!(
                        !ui.archive.identity_section.is_expanded(),
                        "Advanced identity controls start collapsed"
                    );
                    apply_theme(Theme::Dark);
                }
            });
            let weak = Rc::downgrade(self);
            glib::timeout_add_local_once(Duration::from_millis(1400), move || {
                if let Some(ui) = weak.upgrade() {
                    for expander in [
                        &ui.archive.identity_section,
                        &ui.archive.location_section,
                        &ui.archive.launch_section,
                        &ui.archive.advanced_section,
                        &ui.archive.inspection_section,
                    ] {
                        expander.set_expanded(true);
                    }
                    ui.archive.fetch.emit_clicked();
                    let path = ui
                        .state
                        .borrow()
                        .inspection
                        .as_ref()
                        .unwrap()
                        .source
                        .clone();
                    ui.state
                        .borrow_mut()
                        .queue
                        .push_back((path, "browser".into()));
                    ui.update_queue_ui();
                    assert!(
                        ui.archive.next.is_visible(),
                        "Queue control appears only when there is another archive"
                    );
                    ui.state.borrow_mut().queue.clear();
                    ui.update_queue_ui();
                }
            });
            let weak = Rc::downgrade(self);
            if let Some(parent) = std::env::var_os("UNPACK_INSTALL_SMOKE_PARENT") {
                let weak = Rc::downgrade(self);
                glib::timeout_add_local_once(Duration::from_millis(1600), move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.smoke_install_fixture(parent.into());
                    }
                });
            }
            let delay = if std::env::var_os("UNPACK_INSTALL_SMOKE_PARENT").is_some() {
                5
            } else {
                2
            };
            glib::timeout_add_local_once(Duration::from_secs(delay), move || {
                if let Some(ui) = weak.upgrade() {
                    if std::env::var_os("UNPACK_INSTALL_SMOKE_PARENT").is_some() {
                        ui.assert_installed_smoke();
                    }
                    apply_theme(Theme::Light);
                    let bounds = ui
                        .archive
                        .review
                        .compute_bounds(&ui.window)
                        .expect("Install button allocated");
                    assert!(
                        bounds.y() >= 0.0
                            && bounds.y() + bounds.height() <= ui.window.height() as f32,
                        "Install button stays onscreen with all details expanded"
                    );
                    eprintln!("GUI SMOKE PASS: archive name/icon, hidden empty queue, compact Install/Files tabs, top install action at 640px, light/dark themes");
                    ui.window.close();
                }
            });
        }
    }
    pub fn error(&self, message: &str) {
        let dialog = adw::MessageDialog::builder()
            .transient_for(&self.window)
            .heading("Something needs your attention")
            .body(message)
            .build();
        dialog.add_response("close", "Close");
        dialog.set_close_response("close");
        dialog.present();
    }
    pub(super) fn notify(&self, message: &str) {
        if std::env::var_os("UNPACK_SMOKE_TEST").is_some() {
            eprintln!("Smoke notification: {message}");
        }
        self.toast.add_toast(adw::Toast::new(message));
    }
    pub(super) fn begin(&self, message: &str) -> Option<Cancellation> {
        let mut state = self.state.borrow_mut();
        if state.busy {
            drop(state);
            self.notify("Another operation is running. You can cancel it or wait.");
            return None;
        }
        state.busy = true;
        state.cancel = Cancellation::default();
        self.status.set_text(message);
        self.progress.set_visible(true);
        self.progress.pulse();
        self.cancel_button.set_visible(true);
        self.archive.review.set_sensitive(false);
        self.archive.next.set_sensitive(false);
        Some(state.cancel.clone())
    }
    pub(super) fn finish(&self) {
        self.state.borrow_mut().busy = false;
        self.progress.set_visible(false);
        self.cancel_button.set_visible(false);
        self.sync_install_action();
        self.update_queue_ui();
    }
    fn event(self: &Rc<Self>, event: Event) {
        match event {
            Event::Progress(p) => {
                self.status.set_text(&format!(
                    "{} · {} · {}",
                    p.phase,
                    p.path.display(),
                    archive_view::size(p.bytes)
                ));
                if let Some(total) = p.total.filter(|n| *n > 0) {
                    self.progress
                        .set_fraction((p.bytes as f64 / total as f64).min(1.0));
                } else {
                    self.progress.pulse();
                }
            }
            Event::Inspected(inspection, detection) => {
                self.finish();
                self.show_archive(&inspection, &detection);
                {
                    let mut s = self.state.borrow_mut();
                    s.settings.last_directory = inspection.source.parent().map(PathBuf::from);
                    s.inspection = Some(inspection);
                    s.detection = Some(detection);
                }
                self.stack.set_visible_child_name("archive");
                self.archive
                    .pages
                    .set_visible_child_name(if self.state.borrow().prefer_install {
                        "install"
                    } else {
                        "files"
                    });
                self.update_queue_ui();
                self.refresh_archive_icon();
                self.sync_install_action();
                self.status
                    .set_text("Inspection complete · no archive code was executed");
                let queued = self.state.borrow().queue.len();
                if queued > 0 {
                    self.notify(&format!("{queued} more archive(s) queued. Install or extract this archive, then choose Next."));
                }
            }
            Event::Installed(manifest) => {
                {
                    let mut state = self.state.borrow_mut();
                    state.applications.retain(|app| app.id != manifest.id);
                    state.applications.push(manifest.clone());
                }
                self.finish();
                self.status
                    .set_text(&format!("Installed {}", manifest.name));
                self.notify(&format!(
                    "{} is installed. Click Open to launch it.",
                    manifest.name
                ));
                self.refresh_registry();
                // Leave the finished app visible so its primary action is Open.
                // Other archives stay available through the explicit queue button.
            }
            Event::MetadataUpdated(manifest) => {
                self.finish();
                self.notify(&format!("Updated the name and icon for {}", manifest.name));
                self.refresh_registry();
            }
            Event::Registry(apps, history) => {
                self.state.borrow_mut().applications = apps.clone();
                self.show_applications(&apps);
                self.show_history(&history);
                self.sync_install_action();
            }
            Event::Recovered(messages) => {
                if !messages.is_empty() {
                    self.notify(&messages.join("\n"));
                }
                self.refresh_registry();
            }
            Event::BackgroundWarning(message) => self.error(&message),
            Event::Done(message) => {
                self.finish();
                self.status.set_text(&message);
                self.notify(&message);
                self.refresh_registry();
            }
            Event::Failure(message) => {
                self.finish();
                self.status.set_text("Operation stopped");
                self.error(&message);
            }
            Event::ArchiveIcon {
                archive_hash,
                path,
                png,
            } => {
                let current = self
                    .state
                    .borrow()
                    .inspection
                    .as_ref()
                    .is_some_and(|a| a.sha256 == archive_hash)
                    && self.selected_archive_icon().as_ref() == Some(&path)
                    && self.archive.icon.is_active();
                if current {
                    match png {
                        Ok(png) => self
                            .archive
                            .app_icon
                            .set_paintable(Some(&icon_decode::texture(png))),
                        Err(error) => self.notify(&format!("Icon preview unavailable: {error}")),
                    }
                }
            }
        }
    }
    pub fn enqueue(self: &Rc<Self>, path: PathBuf, hint: String) {
        if self.state.borrow().busy
            || self.state.borrow().choosing_open
            || (self.state.borrow().inspection.is_some() && !self.state.borrow().queue.is_empty())
        {
            self.state.borrow_mut().queue.push_back((path, hint));
            self.update_queue_ui();
            self.notify("Archive added to the inspection queue");
            return;
        }
        self.open_archive(path, &hint);
    }
    pub(super) fn next_queued(self: &Rc<Self>) {
        let next = self.state.borrow_mut().queue.pop_front();
        self.update_queue_ui();
        if let Some((path, hint)) = next {
            self.open_archive(path, &hint);
        }
    }
    fn open_archive(self: &Rc<Self>, path: PathBuf, hint: &str) {
        let behavior = if hint == "installer" {
            OpenBehavior::Installer
        } else if hint == "browser" {
            OpenBehavior::Browser
        } else {
            self.state.borrow().settings.open_behavior
        };
        self.state.borrow_mut().prefer_install = behavior != OpenBehavior::Browser;
        if behavior == OpenBehavior::External {
            self.external_archive(path);
            return;
        }
        if behavior == OpenBehavior::Ask {
            self.state.borrow_mut().choosing_open = true;
            let dialog = adw::MessageDialog::builder().transient_for(&self.window).heading("How would you like to open this archive?").body("Browse and extract its contents, or inspect it as a portable application. Nothing inside the archive will be executed.").build();
            dialog.add_responses(&[
                ("cancel", "Cancel"),
                ("external", "Other Archive Manager"),
                ("browse", "Browse Archive"),
                ("install", "Inspect Application"),
            ]);
            dialog.set_default_response(Some("install"));
            dialog.set_close_response("cancel");
            let weak = Rc::downgrade(self);
            dialog.connect_response(None, move |_, response| {
                if let Some(ui) = weak.upgrade() {
                    ui.state.borrow_mut().choosing_open = false;
                    match response {
                        "external" => ui.external_archive(path.clone()),
                        "browse" | "install" => {
                            ui.state.borrow_mut().prefer_install = response == "install";
                            ui.inspect(path.clone());
                        }
                        _ => {}
                    }
                }
            });
            dialog.present();
        } else {
            self.inspect(path);
        }
    }
    fn inspect(self: &Rc<Self>, path: PathBuf) {
        let Some(cancel) = self.begin("Inspecting archive…") else {
            return;
        };
        let limits = self.state.borrow().settings.limits.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let mut progress = progress_sender(sender.clone());
            match archive::inspect(&path, &limits, &cancel, &mut progress) {
                Ok(inspection) => {
                    let detection = detection::analyze(&inspection);
                    let _ = sender.send_blocking(Event::Inspected(inspection, detection));
                }
                Err(e) => {
                    let _ = sender.send_blocking(Event::Failure(e.to_string()));
                }
            }
        });
    }
    fn open_dialog(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder()
            .title("Open Archive")
            .modal(true)
            .build();
        if let Some(path) = &self.state.borrow().settings.last_directory {
            dialog.set_initial_folder(Some(&gio::File::for_path(path)));
        }
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Supported archives"));
        for pattern in [
            "*.tar.gz",
            "*.tgz",
            "*.tar.xz",
            "*.tar.bz2",
            "*.tar.zst",
            "*.tar",
            "*.zip",
            "*.7z",
        ] {
            filter.add_pattern(pattern);
        }
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let all = gtk::FileFilter::new();
        all.set_name(Some("All files"));
        all.add_pattern("*");
        filters.append(&all);
        dialog.set_filters(Some(&filters));
        let ui = self.clone();
        glib::spawn_future_local(async move {
            match dialog.open_multiple_future(Some(&ui.window)).await {
                Ok(files) => {
                    for file in files.iter::<gio::File>().flatten() {
                        if let Some(path) = file.path() {
                            ui.enqueue(path, String::new());
                        }
                    }
                }
                Err(e) if e.matches(gtk::DialogError::Dismissed) => {}
                Err(e) => ui.error(&e.to_string()),
            }
        });
    }
    fn external_archive(self: &Rc<Self>, path: PathBuf) {
        if unpack_core::config::sandboxed_package() {
            self.notify("External archive managers cannot be launched directly from this sandbox. Using the built-in browser.");
            self.inspect(path);
            return;
        }
        let (mime, _) = gio::content_type_guess(Some(&path), None::<&[u8]>);
        let apps: Vec<_> = gio::AppInfo::recommended_for_type(&mime)
            .into_iter()
            .filter(|a| {
                a.id()
                    .is_none_or(|id| !id.starts_with("io.github.unpack.Unpack"))
                    && a.supports_files()
            })
            .collect();
        if apps.is_empty() {
            self.notify("No other archive manager was found. Using Unpack’s built-in browser.");
            self.inspect(path);
            return;
        }
        let dialog = adw::MessageDialog::builder()
            .transient_for(&self.window)
            .heading("Open with another archive manager")
            .body("This does not change your default archive handler.")
            .build();
        dialog.add_response("cancel", "Cancel");
        dialog.set_close_response("cancel");
        for (i, app) in apps.iter().enumerate() {
            dialog.add_response(&i.to_string(), &app.display_name());
        }
        let weak = Rc::downgrade(self);
        dialog.connect_response(None, move |_, response| {
            if let (Some(ui), Ok(index)) = (weak.upgrade(), response.parse::<usize>()) {
                if let Some(app) = apps.get(index) {
                    if let Err(e) =
                        app.launch(&[gio::File::for_path(&path)], Some(&host_launch_context()))
                    {
                        ui.error(&e.to_string());
                    }
                }
            }
        });
        dialog.present();
    }
    pub(super) fn text_window(&self, title: &str, text: &str) {
        let window = adw::Window::builder()
            .transient_for(&self.window)
            .title(title)
            .default_width(680)
            .default_height(520)
            .build();
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        let view = gtk::TextView::builder()
            .editable(false)
            .cursor_visible(true)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .left_margin(18)
            .right_margin(18)
            .top_margin(18)
            .bottom_margin(18)
            .build();
        view.buffer().set_text(text);
        toolbar.set_content(Some(&scroll(&view)));
        window.set_content(Some(&toolbar));
        window.present();
    }
    fn help(&self) {
        self.text_window("Using Unpack", "OPEN · Ctrl+O\nInspect a downloaded archive without executing anything inside it. Browse folders, search, or preview text and README files.\n\nINSTALL · Ctrl+I\nChoose a verified entry point and a user-level destination. Review the exact installation plan, then explicitly confirm. Unpack never runs installation scripts.\n\nEXTRACT · Ctrl+E\nSelect an existing parent folder. Unpack creates a new folder and never merges into or overwrites an existing directory.\n\nMANAGE\nLaunch, inspect, update or remove applications from Installed Applications. Updates require opening the replacement archive and explicitly selecting Update with the same application ID. Modified files stop removal or updates so your changes are preserved. Recoverable backups are retained as hidden .unpack-backup-* siblings.\n\nQUEUE\nOpen multiple archives. Each archive gets its own review and confirmation. Next moves to the following queued archive.\n\nSECURITY\nInspection is not malware detection. Installed applications run with your user account’s permissions when you explicitly launch them. A checksum verifies identity, not authenticity.\n\nSHORTCUTS\nCtrl+O Open · Ctrl+I Review installation · Ctrl+E Extract · Ctrl+F Search · Escape Cancel operation · Ctrl+Q Quit · F1 Help");
    }
    fn about(&self) {
        let dialog = adw::AboutWindow::builder().transient_for(&self.window).application_name("Unpack").application_icon("io.github.unpack.Unpack").version(env!("CARGO_PKG_VERSION")).developer_name("Unpack contributors").license_type(gtk::License::Gpl30).comments("Archives, made approachable.\nTransparent, user-level portable application management.").build();
        dialog.present();
    }
}
pub(super) fn progress_sender(sender: async_channel::Sender<Event>) -> impl FnMut(Progress) {
    let mut last = Instant::now() - Duration::from_secs(1);
    move |progress| {
        if last.elapsed() >= Duration::from_millis(100) {
            let _ = sender.send_blocking(Event::Progress(progress));
            last = Instant::now();
        }
    }
}
pub(super) fn apply_theme(theme: Theme) {
    adw::StyleManager::default().set_color_scheme(match theme {
        Theme::System => adw::ColorScheme::Default,
        Theme::Light => adw::ColorScheme::ForceLight,
        Theme::Dark => adw::ColorScheme::ForceDark,
    });
}
