---
title: Rust, Gtk, Sqlite and Guile
subtitle: generative art generator
author: Johan Hidding
---

This is a lesson in building end-power-user applications in Rust. We'll build a turtle-graphics renderer in Gtk, make it scriptable in Scheme and have it store files in Sqlite. We will build the GUI as much as possible with Glade. In the spirit of reuse, the turtle-graphics system will follow the [Guile tutorial](https://www.gnu.org/software/guile/docs/guile-tut/tutorial.html).

Unfortunately, the Rust bindings for Guile are out-of-date. We'll use the FFI directly to call into Guile 3 with the C API.

# Prelude
The package will be named `gnartgen`, pronounced <span class="phonetic">ˈnɑːt'kɛn</span>, think German dimminuative of a gnarly troll.

``` {.toml file=Cargo.toml}
[package]
name = "gnartgen"
version = "0.1.0"
authors = ["Johan Hidding <j.hidding@esciencecenter.nl>"]
edition = "2018"

[dependencies]
<<cargo-dependencies>>

<<cargo>>
```

Rust has a very nice logging interface. All you need to do is choose your logger back-end. An easy option with nice colors is `pretty_env_logger`.

``` {.toml #cargo-dependencies}
log = "0.4"
pretty_env_logger = "0.4"
glib = "0.10"
```

## Gtk
We'll go ahead and build our entire interface in Glade. This is not the best introduction into Gtk (there are plenty others), but it shows how in general you should approach designing this. We add the following lines to `Cargo.toml`:

``` {.toml #cargo}
[dependencies.gtk]
version = "0.9.0"
features = ["v3_16"]

[dependencies.gio]
version = ""
features = ["v2_44"]
```

To be scripting from within our application we need a code editor:

``` {.toml #cargo}
[dependencies.sourceview]
version = "0.9"
features = ["v3_24"]
```

# Design with Glade
Glade is a very intuitive GUI builder. What I find the tricky bit, is how to connect the design to the code. I made the following design in Glade 3.36. I designed the interface to have a single top-level `GtkApplicationWindow`, with 'client side window decorations' enabled.

On the top, I added a `GtkHeaderBar` containing a `GtkFileChooserButton` and a `Save` button.

The main area is divided in three panes using `GtkPaned`, containing a `GtkTreeView` to list items in the database, a `GtkSourceView` for editing code, and a `GtkDrawingArea` to draw the result in.

The tree view is connected to a `GtkListStore` named `items`, the source view to a buffer named `code_buffer`.

## Build the skeleton application
Now we need to build the application. The basics are explained in [this Gtk-rs Glade tutorial](https://gtk-rs.org/docs-src/tutorial/glade). There is a big problem with combining Gtk and for-instance Sqlite: Gtk wants to live in the main thread, and regulates signals through C-callbacks. These callbacks can only be of type `Box<dyn Fn(...)>`, meaning that any shared state should somehow have a static lifetime. A Sqlite connection is not clonable, so we cannot give access to the connection to multiple callback closures, unless we start putting things in smart pointers. If we then move to a multi-threaded environment, the design breaks again. The best way forward is to setup a messaging channel between the different threads.

- [MPSC Channel API for painless usage of threads with GTK in Rust](https://coaxion.net/blog/2019/02/mpsc-channel-api-for-painless-usage-of-threads-with-gtk-in-rust/)

``` {.rust file=src/main.rs}
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

<<open-file>>

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
```

# Storing files
We store all our data inside a Sqlite database.

``` {.sqlite file=data/schema.sqlite}
create table if not exists "objects"
    ( "id"          integer primary key autoincrement not null
    , "name"        text
    , "description" text
    , "source"      text
    , "thumbnail"   blob
    );
-- vim:ft=mysql
```

``` {.rust #open-file}
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
```

``` {.toml #cargo-dependencies}
rusqlite = { version="0.23", features=["backup"] }
```

