//! Host mode: a page that frames VectorCraft opens documents in it and stores what it saves instead
//! of the browser downloading. The messages are `craft-web-host`'s (storytold/craft-libs), the same
//! contract as the other Craft apps' bridges, with the `vectorcraft:` prefix. This module is
//! VectorCraft's part of it.
//!
//! - `?host=parent` (an `<iframe>`, such as a Nextcloud app) talks to the same-origin parent over
//!   `postMessage`. With no parent, a parent on another origin or a page without an origin (a
//!   `file://` page in some browsers), there is no bridge: the normal app with downloads. The
//!   contract's other transport (`data-host="page"`, `CustomEvent`s on the canvas of a page that
//!   runs the app itself) is not taken: VectorCraft puts a new canvas in the page when it loses its
//!   graphics, and the page's listeners would stay on the old one.
//! - `vectorcraft:ready` (to the host): once, when the app is running and listening; send a file
//!   now. It is not sent again when the app starts again with new graphics (the documents are
//!   still open). VectorCraft reads bytes, so it says `file: false` and a host sends `bytes`.
//! - `vectorcraft:open` (from the host): `{ name, bytes }`, bytes an `ArrayBuffer` or `Uint8Array`,
//!   name a bare file name. A document or a picture (a name ending in an extension File › Open
//!   reads as a document: .vectorcraft, .svg, .svgz, .ai, .eps, .pdf, .png, .psd, .dxf and the
//!   rest) opens as it does picked with File › Open on the desktop, with `name` as its file. Save
//!   then writes a file it can save back (.vectorcraft, .svg, .svgz, .eps, a whole .pdf, a .ai
//!   VectorCraft wrote) under that name and in that format, without the web's Save As dialog; as
//!   on the desktop, it asks first when reading the file left something out. A file in any other
//!   format, and a template, opens without a file of its own, so its first Save asks for a name
//!   and a format. Anything else File › Open takes (a `.wasm` plug-in, presets, libraries,
//!   shortcut sets, scripts), a file of more than 512 MiB and a `File` are refused before they are
//!   copied, with `vectorcraft:failed` ([`crate::host_files`]).
//! - `vectorcraft:save` (to the host): `{ name, saveAs, bytes }` (the bytes a transferred
//!   `ArrayBuffer`) for every document and picture VectorCraft would otherwise download: Save,
//!   Save As, Save a Copy, Save as Template, the exports and the pictures they write. The host
//!   stores the file and reports failures itself; the document counts as saved once the file is
//!   posted. VectorCraft doesn't tell the host which saves make a file the document's, so `saveAs`
//!   is always `false`: a host tells them by the name, which is the opened one for Save. Files
//!   that are not documents (swatch and graphic style libraries, presets, action sets, CSS, zip
//!   packages, preferences) stay browser downloads. While a script keeps the saves in the page
//!   (`window.vectorcraft.captureDownloads`), nothing is posted or downloaded.
//! - `vectorcraft:dirty` whenever unsaved changes appear or go away, in any open document.
//! - `vectorcraft:failed` when the app couldn't start, lost its graphics for good, refused a file
//!   or couldn't open one (which it also says in its own window, as for a picked file).
//!
//! In host mode the host guards leaving the page, and Data Recovery keeps no copies: they would
//! stay in the browser storage of the host's site, where the next person to use that browser is
//! offered them (`web.rs`).

use std::cell::RefCell;
use std::rc::Rc;

use craft_web_host::Mode;
use craft_web_host::web::{FileData, Host, Message};
use vectorcraft_engine::cmd::fileio;
use vectorcraft_ui_egui::{VectorcraftApp, i18n, io, unsaved};

use crate::host_files;

/// The app id in the host messages (`vectorcraft:ready`).
const APP: &str = "vectorcraft";

thread_local! {
    /// The bridge [`deliver`] posts through: the download hook is a plain function, and the
    /// services that call it are made again when the graphics restart.
    static BRIDGE: RefCell<Option<Bridge>> = const { RefCell::new(None) };
}

/// A file the host sent: its name and its bytes.
type Arrived = (String, Vec<u8>);

#[derive(Clone)]
pub struct Bridge {
    host: Rc<Host>,
    /// Files the host sent, waiting for the next frame.
    opened: Rc<RefCell<Vec<Arrived>>>,
    /// The context a file's arrival wakes: the running one, which changes when the graphics restart.
    ctx: Rc<RefCell<Option<egui::Context>>>,
}

impl Bridge {
    /// The bridge for this page, which [`deliver`] posts through from now on, or `None` when
    /// VectorCraft isn't hosted: no `?host=parent`, an unframed `?host=parent` page (it stays the
    /// normal app with downloads), or a parent on another origin.
    pub fn connect(canvas: &web_sys::HtmlCanvasElement) -> Option<Self> {
        let host = Host::connect(APP, canvas)?;
        // The canvas is replaced when the graphics are lost, and events on it would go with it.
        if host.mode() != Mode::Parent {
            return None;
        }
        // An opaque origin (a file:// page in some browsers) can't be a postMessage target.
        if web_sys::window()?.location().origin().ok()? == "null" {
            return None;
        }
        let bridge = Self { host: Rc::new(host), opened: Rc::default(), ctx: Rc::default() };
        BRIDGE.with(|b| *b.borrow_mut() = Some(bridge.clone()));
        Some(bridge)
    }

    /// Wake `ctx` when a file arrives: the app runs on a new context after losing its graphics.
    pub fn set_context(&self, ctx: &egui::Context) {
        *self.ctx.borrow_mut() = Some(ctx.clone());
    }

    /// Hear `vectorcraft:open` and queue the file for the next frame. A host's file is untrusted
    /// input: what is not a document, what is too large and what is not bytes is refused before
    /// it is copied into the app's memory ([`host_files::refusal`], from the data's own length,
    /// not the host's claim). The crate has cleaned the name (and cut it to 255 characters).
    /// VectorCraft's saves carry no id, so nothing else the host says changes what it does.
    pub fn listen(&self) {
        let bridge = self.clone();
        self.host.listen(move |message| {
            let Message::Open { name, data } = message else { return };
            let waiting: u64 = bridge.opened.borrow().iter().map(|(_, bytes)| bytes.len() as u64).sum();
            let bytes = match host_files::refusal(&name, matches!(data, FileData::Bytes(_)), data.size(), waiting) {
                None => copied(&data).ok_or_else(|| "there is not enough memory left for it".to_string()),
                Some(why) => Err(why),
            };
            match bytes {
                Ok(bytes) => {
                    bridge.opened.borrow_mut().push((name, bytes));
                    if let Some(ctx) = bridge.ctx.borrow().as_ref() {
                        ctx.request_repaint();
                    }
                }
                Err(why) => {
                    log::warn!("vectorcraft-web: {name} from the host page was not opened: {why}");
                    bridge.post_failed(&couldnt_open(&name, &why));
                }
            }
        });
    }

    /// The app is running and listening: the host sends the document now.
    pub fn post_ready(&self) {
        if let Err(e) = self.host.post_ready(env!("CARGO_PKG_VERSION"), false) {
            log::error!("vectorcraft-web: couldn't tell the host page the app is ready: {e}");
        }
    }

    /// The app couldn't start or couldn't take a file: let the host say so.
    pub fn post_failed(&self, error: &str) {
        if let Err(e) = self.host.post_failed(error, None) {
            log::error!("vectorcraft-web: couldn't tell the host page “{error}”: {e}");
        }
    }

    /// Each frame: open the files the host sent, as File › Open opens a picked file on the
    /// desktop. A file keeps its name as its path, which is what lets Save write it back to the
    /// host under that name and in its format with no dialog: the engine keeps the path only for a
    /// file Save can write back, and nothing reads from it here, where there is no file system. A
    /// file that can't be opened is reported as a picked one is, and to the host.
    pub fn open_arrived(&self, app: &mut VectorcraftApp) {
        let arrived = std::mem::take(&mut *self.opened.borrow_mut());
        for (name, bytes) in arrived {
            if let Err(e) = io::open_bytes(app, &name, &bytes, Some(name.clone())) {
                io::report_open_error(app, &name, &e);
                self.post_failed(&couldnt_open(&name, &e));
            }
        }
    }

    /// Each frame: tell the host when unsaved changes appear or go away (the crate does not repeat
    /// a state it already reported).
    pub fn report_dirty(&self, app: &VectorcraftApp) {
        self.host.post_dirty(unsaved::any_dirty(app));
    }
}

/// The download hook in host mode: hand the host a file the app would otherwise download. `None`
/// when the browser downloads it after all: VectorCraft isn't hosted, or the file is not a
/// document or a picture ([`host_files::goes_to_page`]). `Some(Ok)` once the file was posted,
/// which is when a document counts as saved (the saves carry no id, so the host doesn't confirm
/// them); an `Err` fails the save like a download that couldn't start.
pub fn deliver(path: &str, bytes: &[u8]) -> Option<Result<(), String>> {
    let bridge = BRIDGE.with(|b| b.borrow().clone())?;
    let name = fileio::file_name(path);
    host_files::goes_to_page(&name).then(|| bridge.host.post_save(&name, bytes, None, false))
}

/// The host's bytes copied into the app's memory, or `None` when it has no room for them (or they
/// came as a `File`, which is refused before). An allocation that fails would abort the app, so
/// the room is asked for first.
fn copied(data: &FileData) -> Option<Vec<u8>> {
    let FileData::Bytes(array) = data else { return None };
    let len = usize::try_from(array.length()).ok()?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).ok()?;
    bytes.resize(len, 0);
    array.copy_to(&mut bytes);
    Some(bytes)
}

/// What the host is told of a file that didn't open: the status bar's own line for it, in the
/// interface language where the catalogs have its parts.
fn couldnt_open(name: &str, why: &str) -> String {
    i18n::message(i18n::current(), &format!("Couldn't open {name}: {why}"))
}
