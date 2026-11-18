//! VectorCraft in the browser.
//!
//! Runs the same [`vectorcraft_ui_egui::VectorcraftApp`] as the desktop app through eframe's web
//! runner (wgpu: WebGPU where available, WebGL2 otherwise). Build with `trunk build --release`
//! from this directory; see `docs/development.md` ("Web build").
//!
//! Differences from the desktop app:
//! - no TCP control server (browsers can't listen on sockets);
//! - File → Open uses the browser file picker; bytes arrive asynchronously through
//!   `Services::picked_inbox`;
//! - Save / Export trigger a browser download;
//! - dropped files are read asynchronously by `web::WebShell` and delivered through the inbox;
//! - pen and finger input is read from Pointer Events by `pencil` (Apple Pencil pressure and
//!   hover, palm rejection, fingers navigate once a pen has been seen, the pen's eraser end
//!   selects the Eraser tool);
//! - a Cmd or Ctrl chord the keymap binds runs its command, not the browser's own action as well
//!   (`Cmd+=` zooms the canvas, not the page; `Cmd+Z` undoes in the document) (`keys`).
//!
//! URL query flags: `?webgl` forces the WebGL2 backend instead of WebGPU; `?lang=es` (any
//! language code) stands in for the browser's languages when the interface language is Automatic;
//! `?host=parent` (inside an `<iframe>`) lets the same-origin page that frames the app send the
//! documents to open and store what the app saves instead of the browser downloading it, over
//! the Craft apps' host messages (`host`: `vectorcraft:ready`, `open`, `save`, `dirty`, `failed`).
//! Hosted, the app opens only documents and pictures from the page, hands it the documents and
//! pictures it writes (other files are still downloaded), and keeps no Data Recovery copies.

#[cfg(target_arch = "wasm32")]
mod api;
#[cfg(target_arch = "wasm32")]
mod fonts;
#[cfg(target_arch = "wasm32")]
mod host;
#[cfg(any(target_arch = "wasm32", test))]
mod host_files;
#[cfg(any(target_arch = "wasm32", test))]
mod keys;
#[cfg(any(target_arch = "wasm32", test))]
mod locks;
#[cfg(any(target_arch = "wasm32", test))]
mod pencil;
#[cfg(any(target_arch = "wasm32", test))]
mod saved_files;
#[cfg(target_arch = "wasm32")]
mod web;

/// Whether the address's query (`?webgl&lang=it`) has the flag `flag`, bare or with a value. Only
/// a whole parameter counts: a page that hosts the app puts free text in the query (a file's or a
/// person's name), and a word in it is not a flag.
#[cfg(any(target_arch = "wasm32", test))]
fn has_flag(query: &str, flag: &str) -> bool {
    query.trim_start_matches('?').split('&').any(|pair| pair.split('=').next() == Some(flag))
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("vectorcraft-web only runs in the browser: build it with `trunk build --release` in apps/vectorcraft-web");
}

#[cfg(test)]
mod tests {
    use super::has_flag;

    #[test]
    fn a_url_flag_is_a_whole_query_parameter() {
        assert!(has_flag("?webgl", "webgl"));
        assert!(has_flag("?lang=it&webgl", "webgl"));
        assert!(has_flag("?webgl=1&host=parent", "webgl"));
        assert!(!has_flag("", "webgl"));
        assert!(!has_flag("?nowebgl", "webgl"));
        // What a hosting page puts in the query is not a flag, whatever it says.
        assert!(!has_flag("?lang=it&host=parent&author=webgl&v=2", "webgl"));
        assert!(!has_flag("?host=parent&author=Ada%20webgl", "webgl"));
    }
}
