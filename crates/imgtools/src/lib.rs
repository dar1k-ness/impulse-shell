//! In-process ports of the original's image scripts, so the shell starts no script and no Python venv
//! for them. `busy`, `regions` and `textcolor` reproduce their script's output exactly (same
//! OpenCV algorithms, ported); `thumbs` writes the thumbnails the original's scripts wrote, under the
//! names `ThumbnailImage.qml` looks up.

pub mod busy;
pub mod cv;
pub mod cv_resize;
pub mod limit;
pub mod magick;
pub mod pool;
pub mod regions;
pub mod textcolor;
pub mod thumbs;
