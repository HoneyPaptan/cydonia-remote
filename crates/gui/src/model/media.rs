//! Pictures a document points at: where a pasted screenshot's bytes go, and
//! how the ones a message carries reach an agent.
//!
//! A document holds a URL, and bytes off the clipboard have no address
//! anywhere — so somewhere has to be picked before an image block can exist,
//! which is what `editor::set_image_store` asks the app. The answer is the
//! open article's own `assets/`, the same directory agents are told to write
//! into, so what the app pastes and what an agent generates land together and
//! a picture is named for its bytes wherever it came from. A session's
//! attachments go to the project's shared `assets/` instead — a transcript is
//! not an article.
//!
//! The editor names which document is asking, but what it hands over is an
//! `Entity<Editor>` and the article behind one is the workspace's to know —
//! so which directory to write into is still noted by [`aim`] as a document
//! opens. One window and one document in front of it, so one target.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use bezel::gpui::{App, Entity, Image, hash};
use editor::{Editor, ImageStore, Source};
use image::{ImageFormat, imageops::FilterType};
use markdown::BlockKind;
use std::{
    borrow::Cow,
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
};

/// What a picture's file name begins with.
const MARK: &str = "media-";

/// The longest edge a picture is sent to an agent at. Larger ones are scaled
/// down on the way out, since a model reads no more than this and the base64
/// of a retina screenshot is megabytes of prompt.
pub const LONG_EDGE: u32 = 1568;

/// A picture picked in the composer, held until the message is sent — the
/// composer does not know which project the session it feeds is in.
#[derive(Clone)]
pub enum Attachment {
    Bytes(Arc<Image>),
    File(PathBuf),
    Upload { name: String, bytes: Arc<[u8]> },
}

impl Attachment {
    pub fn name(&self) -> String {
        match self {
            Attachment::Bytes(image) => format!("image.{}", image.format.extension()),
            Attachment::File(path) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            Attachment::Upload { name, .. } => name.clone(),
        }
    }

    pub fn is_picture(&self) -> bool {
        match self {
            Attachment::Bytes(_) => true,
            _ => markdown::is_image(&self.name()),
        }
    }

    pub fn bytes(&self) -> Option<Cow<'_, [u8]>> {
        match self {
            Attachment::Bytes(image) => Some(Cow::Borrowed(&image.bytes)),
            Attachment::File(path) => std::fs::read(path).ok().map(Cow::Owned),
            Attachment::Upload { bytes, .. } => Some(Cow::Borrowed(bytes)),
        }
    }
}

/// Install the store. Called once, beside the other `init`s.
///
/// `accepts` is left at the editor's own guess from the extension — cydonia
/// decodes nothing the default would turn away.
pub fn init(cx: &mut App) {
    editor::set_image_store(
        cx,
        ImageStore {
            keep,
            ..ImageStore::default()
        },
    );
}

/// Write `bytes` into `dir` under a name taken from their hash, so the same
/// picture kept twice is the one file — and gpui, which caches a decoded
/// picture against its path, is handed the copy it already has.
pub fn store(dir: &Path, bytes: &[u8], extension: &str) -> Option<PathBuf> {
    write_once(dir, format!("{MARK}{:x}.{extension}", hash(&bytes)), bytes)
}

fn write_once(dir: &Path, name: String, bytes: &[u8]) -> Option<PathBuf> {
    let file = dir.join(name);
    if !file.is_file() {
        std::fs::create_dir_all(dir).ok()?;
        std::fs::write(&file, bytes).ok()?;
    }
    Some(file)
}

/// Keep an attachment in `dir`. A file is copied in rather than pointed at
/// where it is: a transcript that outlives the download it was sent from is
/// the reason the picture lives with the project.
pub fn keep_attachment(dir: &Path, attachment: &Attachment) -> Option<PathBuf> {
    let bytes = attachment.bytes()?;
    let name = attachment.name();
    match attachment.is_picture() {
        true => store(dir, &bytes, &extension(Path::new(&name))?),
        false => store_document(dir, &bytes, &name),
    }
}

fn store_document(dir: &Path, bytes: &[u8], name: &str) -> Option<PathBuf> {
    let plain: String = name
        .chars()
        .map(|c| match c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
            true => c,
            false => '_',
        })
        .collect();
    let plain = plain.trim_start_matches('.');
    let plain = match plain.is_empty() {
        true => "file",
        false => plain,
    };
    write_once(dir, format!("{MARK}{:x}-{plain}", hash(&bytes)), bytes)
}

/// A picture as a message line. The destination is bracketed, since a
/// project path is free to have a space in it.
pub fn line(path: &Path) -> String {
    match markdown::is_image(&path.to_string_lossy()) {
        true => format!("![](<{}>)", path.display()),
        false => format!("[{}](<{}>)", document_title(path), path.display()),
    }
}

fn document_title(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let shown = name
        .strip_prefix(MARK)
        .and_then(|rest| rest.split_once('-'))
        .map_or(name.as_str(), |(_, original)| original);
    shown.replace(['[', ']'], "")
}

/// The local pictures a message points at, in order.
pub fn attached(text: &str) -> Vec<PathBuf> {
    markdown::parse(text)
        .blocks
        .into_iter()
        .filter_map(|block| match block.kind {
            BlockKind::Image { url, .. } if !url.is_empty() && !url.contains("://") => {
                Some(PathBuf::from(url))
            }
            _ => None,
        })
        .collect()
}

/// Restore local image blocks as composer attachments.
pub fn detach(text: &str) -> (String, Vec<PathBuf>) {
    let mut doc = markdown::parse(text);
    let mut attachments = Vec::new();
    doc.blocks.retain(|block| {
        if let BlockKind::Image { url, .. } = &block.kind
            && !url.is_empty()
            && !url.contains("://")
        {
            attachments.push(PathBuf::from(url));
            return false;
        }
        true
    });
    let text = if attachments.is_empty() {
        text.to_owned()
    } else {
        markdown::serialize(&doc)
    };
    (text, attachments)
}

/// A picture as an agent is handed one: base64 and its MIME type. Sent as it
/// is when it is already small and in a format models read, and otherwise
/// scaled to [`LONG_EDGE`] and written as a PNG.
pub fn encode(path: &Path) -> Option<(String, &'static str)> {
    let bytes = std::fs::read(path).ok()?;
    let format = image::guess_format(&bytes).ok()?;
    let picture = image::load_from_memory_with_format(&bytes, format).ok()?;
    let fits = picture.width().max(picture.height()) <= LONG_EDGE;
    let mime = match format {
        ImageFormat::Png => Some("image/png"),
        ImageFormat::Jpeg => Some("image/jpeg"),
        ImageFormat::Gif => Some("image/gif"),
        ImageFormat::WebP => Some("image/webp"),
        _ => None,
    };
    if let (true, Some(mime)) = (fits, mime) {
        return Some((STANDARD.encode(&bytes), mime));
    }
    let picture = match fits {
        true => picture,
        false => picture.resize(LONG_EDGE, LONG_EDGE, FilterType::Lanczos3),
    };
    let mut out = Cursor::new(Vec::new());
    picture.write_to(&mut out, ImageFormat::Png).ok()?;
    Some((STANDARD.encode(out.into_inner()), "image/png"))
}

fn extension(path: &Path) -> Option<String> {
    Some(path.extension()?.to_str()?.to_ascii_lowercase())
}

/// Take a picture into the open project's assets, and answer with what the
/// document is to point at.
///
/// An absolute path rather than a relative one: what paints the picture reads
/// the URL as a path off this process, whose working directory is not the
/// project's.
/// Into the `assets/` beside the editor's base, answered relative to it: an
/// article's own folder, or for a card its project's `.cydonia`, so a card's
/// pictures land in the project's shared `assets/`. An editor with no base
/// lets the picture go.
fn keep(source: Source, _: &Entity<Editor>, base: Option<&Path>, _: &App) -> Option<String> {
    let base = base?.to_path_buf();
    let dir = artifact::article::assets(&artifact::article::content(&base));
    let (bytes, extension) = match source {
        Source::Bytes(image) => (
            Cow::Borrowed(image.bytes.as_slice()),
            image.format.extension().to_owned(),
        ),
        Source::File(path) => (Cow::Owned(std::fs::read(path).ok()?), extension(path)?),
    };
    let file = store(&dir, &bytes, &extension)?;
    let file = file.strip_prefix(&base).unwrap_or(&file);
    Some(file.to_string_lossy().into_owned())
}
