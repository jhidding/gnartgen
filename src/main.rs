// ~\~ language=Rust filename=src/main.rs
// ~\~ begin <<lit/index.md|src/main.rs>>[0]
extern crate pretty_env_logger;
extern crate gtk;
extern crate gio;
extern crate sourceview;
extern crate rusqlite;
extern crate glib;

use gtk::prelude::*;
use sourceview::prelude::*;
use std::path::Path;
use rusqlite::{Connection};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::rc::Rc;
use std::cell::RefCell;
use glib::clone;

#[derive(Debug)]
enum Error {
    SQL(rusqlite::Error),
    User(String),
}

type Result<T, E=Error> = std::result::Result<T, E>;
// use gio::prelude::*;

const SCHEMA : &str = std::include_str!("../data/schema.sqlite");

fn init_code_buffer(view: &sourceview::View) -> sourceview::Buffer {
    use sourceview::{Buffer,StyleSchemeManager,LanguageManager};
    let language_manager = LanguageManager::get_default()
        .expect("Default language manager");
    let scheme_language = language_manager.get_language("scheme")
        .expect("Scheme language");
    let theme_manager = StyleSchemeManager::get_default()
        .expect("Default style scheme manager");
    let theme = theme_manager.get_scheme("tango")
        .expect("Tango style scheme");
    let code_buffer = Buffer::new_with_language(&scheme_language);
    code_buffer.set_style_scheme(Some(&theme));
    view.set_buffer(Some(&code_buffer));
    code_buffer
}

// ~\~ begin <<lit/index.md|open-file>>[0]
fn open_file<P: AsRef<Path>>(path: P) -> Result<Connection> {
    let conn = Connection::open(path).map_err(Error::SQL)?;
    conn.execute_batch(&SCHEMA).map_err(Error::SQL)?;
    Ok(conn)
}

fn save_file<P: AsRef<Path>>(conn: &mut Connection, path: P) -> Result<()> {
    use rusqlite::DatabaseName::*;
    use rusqlite::backup::Progress;
    conn.backup(Main, path, None::<fn(Progress)>).map_err(Error::SQL)
}

fn new_project() -> Result<Connection> {
    let conn = Connection::open_in_memory().map_err(Error::SQL)?;
    let schema = std::include_str!("../data/schema.sqlite");
    conn.execute_batch(&schema).map_err(Error::SQL)?;
    Ok(conn)
} 

struct State {
    pub builder: gtk::Builder,
    open_file: Option<String>,
    conn: Connection,
}

impl State {
    fn new(builder: gtk::Builder) -> State {
        let conn = new_project().unwrap();
        State { open_file: None, builder: builder, conn: conn }
    }

    fn open(&mut self, path: String) -> Result<()> {
        let conn = open_file(&path)?;
        log::info!("Loaded {}.", path);
        self.conn = conn;
        self.open_file = Some(path);
        Ok(())
    }

    fn save_as(&mut self, path: String) -> Result<()> {
        save_file(&mut self.conn, &path)?;
        self.open(path)
    }

    fn set_title(&self) {
        let header: gtk::HeaderBar = self.builder.get_object("header").unwrap();

        header.set_title(self.open_file.as_ref().map(|x| x.as_str()));
    }
}
// ~\~ end

fn select_file_dialog(w: &gtk::Window) -> Option<std::path::PathBuf> {
    let dialog = gtk::FileChooserDialog::with_buttons(
        Some("Open File"), Some(w), gtk::FileChooserAction::Save,
        &[("_Cancel", gtk::ResponseType::Cancel), ("_Open", gtk::ResponseType::Accept)]);
    let result = match dialog.run() {
        gtk::ResponseType::Cancel => None,
        gtk::ResponseType::Accept => dialog.get_filename(),
        _                         => None
    };
    unsafe { dialog.destroy(); }
    result
}

fn main() {
    pretty_env_logger::init();
    if gtk::init().is_err() {
        println!("Failed to initialize GTK.");
        return;
    }
    sourceview::View::static_type();

    log::debug!("building Glade components");
    let glade_src = std::include_str!("../data/gnartgen.glade");
    let builder = gtk::Builder::from_string(glade_src);
    let window: gtk::Window = builder.get_object("window").unwrap();
    log::debug!("creating code buffer");
    let code_view: sourceview::View = builder.get_object("code_view").unwrap();
    init_code_buffer(&code_view);

    // let (sink, source) = channel();
    //let mut state = State::new();
    let state = Rc::new(RefCell::new(State::new(builder.clone())));

    builder.connect_signals(|_, signal_name| {
        // let psink = sink.clone();
        match signal_name {
            "on_window_destroy" => {
                log::debug!("Connecting on_window_destroy signal");
                Box::new(|_| {
                    log::info!("Bye!");
                    gtk::main_quit(); None
                })
            }
            "open_button_clicked" => {
                log::debug!("Connecting on_button_clicked signal");
                Box::new(clone!(@strong state => move |w| {
                    let window = w[0].get::<gtk::Button>().unwrap().unwrap().get_toplevel().unwrap().downcast::<gtk::Window>().unwrap();
                    let path = select_file_dialog(&window);
                    if path.is_none() { return None; }
                    let path_str = path.unwrap().to_str().unwrap().to_string();
                    log::info!("Open file: {}", path_str);
                    let err : Result<()>; //  = Ok(());
                    err = (*state).borrow_mut().open(path_str);
                    match err {
                        Ok(()) => { (*state).borrow().set_title(); }
                        Err(e) => { log::warn!("{:?}", e); }
                    };
                    // psink.send(Msg::Open(path_str)).unwrap();
                    None
                }))
            }
            // "on_cancel_clicked" => Box::new(|w| {
            //     let 
            _ => {
                log::debug!("Not connecting to {} signal", signal_name);
                Box::new(|_| { None })
            }
        }
    });
    window.show_all();
    gtk::main();
}
// ~\~ end
