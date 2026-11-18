//! Which files pass between VectorCraft and the page that hosts it (`host.rs`): the files the page
//! may open, and the files the app writes that the page is given. Both go by the file's name, so
//! they are decided before a file's bytes are copied or posted.

use vectorcraft_engine::cmd::fileio;
use vectorcraft_ui_egui::io;

/// The most the page's files may take while they wait to open, so also the largest file taken from
/// the page. A page's file is untrusted input, and the web build has 4 GiB to address and aborts
/// when an allocation fails: a larger file is refused before it is copied into the app's memory.
/// File › Open has no limit of its own.
pub const MAX_OPEN_BYTES: u64 = 512 << 20;

/// Does a file named `name` open from the page? Only a document or a picture does: a name whose
/// extension one of the app's own readers lists (`fileio::OPEN_EXTS`; importer plug-ins are not
/// asked).
///
/// [`io::open_bytes`], which opens the file, also installs a `.wasm` plug-in, imports presets,
/// libraries and composite fonts, reviews a shortcut set and checks a script, each told by the
/// file's extension. A page must never get to do those, so a name it would take that way is
/// refused here whatever else it names. A name without an extension is refused too: nothing says
/// what the file is before it is read.
pub fn opens_from_page(name: &str) -> bool {
    fileio::readable_format_for_name(name).is_some() && io::opens_as(name).is_none()
}

/// Why the page's file `name` is not taken, or `None` when it is. `bytes`: the page sent bytes,
/// not a `File` to read in slices; `size`: their length; `waiting`: the bytes of the page's files
/// that have not opened yet (they open on the next frame, and a hidden tab draws none).
pub fn refusal(name: &str, bytes: bool, size: f64, waiting: u64) -> Option<String> {
    let most = MAX_OPEN_BYTES as f64;
    if !opens_from_page(name) {
        Some("a page that hosts VectorCraft opens only documents and pictures in it, told by the extension of the file's name".into())
    } else if !bytes {
        Some("the page sent it as a File, and VectorCraft reads bytes".into())
    } else if size.is_nan() || size > most {
        Some(format!("it is larger than {} MiB, the most a page opens", MAX_OPEN_BYTES >> 20))
    } else if size + waiting as f64 > most {
        Some(format!("the files the page sent that have not opened yet would take more than {} MiB", MAX_OPEN_BYTES >> 20))
    } else {
        None
    }
}

/// Is the file the app writes as `name` handed to the page? A document or a picture is: a name
/// whose extension is one of the app's formats (`fileio::FORMATS`), which is what Save, Save As,
/// Save a Copy and the exports write. Any other file the app writes is not a document (a swatch or
/// graphic style library, a Libraries panel library, presets, an action set, composite fonts, CSS,
/// a zip package, preferences): a page keeps documents, so those stay browser downloads.
pub fn goes_to_page(name: &str) -> bool {
    fileio::format_for_name(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_and_pictures_open_from_the_page() {
        // Every extension File › Open reads as a document, in any letter case.
        for ext in fileio::OPEN_EXTS {
            assert!(opens_from_page(&format!("Poster.{ext}")), ".{ext}");
            assert!(opens_from_page(&format!("POSTER.{}", ext.to_uppercase())), ".{ext} in capitals");
        }
        // The last extension decides, as it does when the file opens.
        assert!(opens_from_page("plug-in.wasm.svg"));
        assert!(opens_from_page("../logo v2.final.ai"));
    }

    #[test]
    fn nothing_file_open_takes_another_way_opens_from_the_page() {
        // Everything File › Open's dialog lists is either a document, which opens, or a file it
        // installs, imports, checks or reviews, which doesn't: none is both, none is neither.
        for (label, exts) in fileio::open_filters() {
            for ext in exts {
                let name = format!("file.{ext}");
                assert_eq!(opens_from_page(&name), !fileio::opens_as_other(ext), "{label}: {name}");
            }
        }
        let refused = [
            "filter.wasm",
            "FILTER.WASM",
            "keys.kys",
            "run.jsx",
            "lib.jsxinc",
            "packed.jsxbin",
            "brand.ase",
            "brand.vcswatches",
            "brand.gpl",
            "styles.vcstyles",
            "serif.vccomposite",
            "actions.aia",
            // Read by Place, never opened as documents.
            "notes.txt",
            "letter.docx",
            // Not VectorCraft's at all.
            "site.html",
            "package.zip",
            "logo.svg.exe",
            // Nothing says what these are.
            "document",
            "logo.svg.",
            ".svg",
            "",
        ];
        for name in refused {
            assert!(!opens_from_page(name), "{name:?}");
            assert!(refusal(name, true, 1.0, 0).is_some(), "{name:?}");
        }
    }

    #[test]
    fn a_file_is_refused_before_it_is_copied() {
        let most = MAX_OPEN_BYTES as f64;
        assert_eq!(refusal("logo.svg", true, 0.0, 0), None);
        assert_eq!(refusal("logo.svg", true, most, 0), None);
        assert!(refusal("logo.svg", true, most + 1.0, 0).is_some_and(|why| why.contains("512 MiB")));
        assert!(refusal("logo.svg", true, f64::INFINITY, 0).is_some());
        assert!(refusal("logo.svg", true, f64::NAN, 0).is_some());
        // A `File` would be read in slices, which VectorCraft doesn't do.
        assert!(refusal("logo.svg", false, 10.0, 0).is_some_and(|why| why.contains("File")));
        // Files that wait to open count too: the page can't fill the memory while no frame is drawn.
        assert_eq!(refusal("logo.svg", true, 10.0, MAX_OPEN_BYTES - 10), None);
        assert!(refusal("logo.svg", true, 11.0, MAX_OPEN_BYTES - 10).is_some_and(|why| why.contains("not opened yet")));
        // What the file is comes first: a plug-in is refused as one, whatever its size.
        assert!(refusal("filter.wasm", true, most + 1.0, 0).is_some_and(|why| why.contains("documents and pictures")));
    }

    #[test]
    fn documents_and_pictures_go_to_the_page_and_other_files_are_downloaded() {
        // What Save, Save As and Save as Template write, and what the exports write.
        let saved = fileio::FORMATS.iter().filter(|f| fileio::SAVE_FORMATS.contains(&f.id) || fileio::TEMPLATE_FORMATS.contains(&f.id));
        for ext in saved.flat_map(|f| f.extensions).chain(&fileio::export_extensions()) {
            assert!(goes_to_page(&format!("Poster.{ext}")), ".{ext}");
        }
        for name in ["logo.vectorcraft", "LOGO.SVG", "logo.svgz", "logo.ai", "logo.eps", "logo.pdf", "logo@2x.png", "photo.jpeg", "out/logo.tif"] {
            assert!(goes_to_page(name), "{name}");
        }
        let downloaded = [
            "Brand.vcswatches",
            "Brand.ase",
            "Brand.gpl",
            "Styles.vcstyles",
            "Serif.vccomposite",
            "Actions.vcactions",
            "keys.kys",
            "Poster.css",
            "Poster.html",
            "Poster.zip",
            "VectorCraft Shortcuts.json",
            "Poster",
            "",
        ];
        for name in downloaded {
            assert!(!goes_to_page(name), "{name:?}");
        }
        // The presets and libraries File › Open imports are written under those extensions too.
        for (label, exts) in fileio::open_filters() {
            for ext in exts.iter().filter(|ext| fileio::opens_as_other(ext)) {
                assert!(!goes_to_page(&format!("file.{ext}")), "{label}: .{ext}");
            }
        }
    }
}
