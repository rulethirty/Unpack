mod ui;

use adw::prelude::*;
use gtk::gio;
use std::{cell::RefCell, rc::Rc};

fn main() -> gtk::glib::ExitCode {
    let app = adw::Application::builder()
        .application_id("io.github.unpack.Unpack")
        .flags(if std::env::var_os("UNPACK_SMOKE_TEST").is_some() {
            gio::ApplicationFlags::HANDLES_COMMAND_LINE
                | gio::ApplicationFlags::HANDLES_OPEN
                | gio::ApplicationFlags::NON_UNIQUE
        } else {
            gio::ApplicationFlags::HANDLES_COMMAND_LINE | gio::ApplicationFlags::HANDLES_OPEN
        })
        .build();
    app.add_main_option(
        "browse",
        gtk::glib::Char::from(b'b'),
        gtk::glib::OptionFlags::NONE,
        gtk::glib::OptionArg::None,
        "Browse archives without the opening-behavior prompt",
        None,
    );
    app.add_main_option(
        "install",
        gtk::glib::Char::from(b'i'),
        gtk::glib::OptionFlags::NONE,
        gtk::glib::OptionArg::None,
        "Inspect archives as portable applications",
        None,
    );
    let window: Rc<RefCell<Option<Rc<ui::Window>>>> = Rc::new(RefCell::new(None));
    let slot = window.clone();
    app.connect_activate(move |app| {
        if let Some(window) = slot.borrow().as_ref() {
            window.window.present();
            return;
        }
        let ui = ui::Window::new(app);
        ui.window.present();
        *slot.borrow_mut() = Some(ui.clone());
        ui.startup();
    });
    let slot = window.clone();
    app.connect_open(move |app, files, hint| {
        app.activate();
        if let Some(ui) = slot.borrow().as_ref() {
            for file in files {
                if let Some(path) = file.path() { ui.enqueue(path, hint.to_string()); }
                else { ui.error("Only local archive files are supported. Download this file before opening it."); }
            }
        }
    });
    let slot = window.clone();
    app.connect_command_line(move |app, command| {
        app.activate();
        let options = command.options_dict();
        let hint = if options.contains("browse") {
            "browser"
        } else if options.contains("install") {
            "installer"
        } else {
            ""
        };
        if let Some(ui) = slot.borrow().as_ref() {
            for argument in command.arguments().into_iter().skip(1) {
                if argument == "--browse"
                    || argument == "-b"
                    || argument == "--install"
                    || argument == "-i"
                    || argument == "--"
                {
                    continue;
                }
                let file = command.create_file_for_arg(&argument);
                if let Some(path) = file.path() {
                    ui.enqueue(path, hint.into());
                } else {
                    ui.error("Only local archive files are supported");
                }
            }
        }
        gtk::glib::ExitCode::SUCCESS
    });
    app.set_accels_for_action("win.open", &["<Primary>o"]);
    app.set_accels_for_action("win.install", &["<Primary>i"]);
    app.set_accels_for_action("win.extract", &["<Primary>e"]);
    app.set_accels_for_action("win.search", &["<Primary>f"]);
    app.set_accels_for_action("win.cancel", &["Escape"]);
    app.set_accels_for_action("win.help", &["F1"]);
    app.set_accels_for_action("app.quit", &["<Primary>q"]);
    let quit = gio::SimpleAction::new("quit", None);
    let weak = app.downgrade();
    quit.connect_activate(move |_, _| {
        if let Some(app) = weak.upgrade() {
            if let Some(window) = app.active_window() {
                window.close();
            } else {
                app.quit();
            }
        }
    });
    app.add_action(&quit);
    app.run()
}
