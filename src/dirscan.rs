use std::path::{Path, PathBuf};

const SUPPORTED_EXTENSIONS: &[&str] =
    &["jpg", "jpeg", "png", "apng", "webp", "gif", "heic", "heif"];

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Sibling images in `dir`, naturally sorted by filename, plus the index of `current`.
pub struct DirListing {
    pub files: Vec<PathBuf>,
    pub current_index: usize,
}

pub fn scan(current: &Path) -> DirListing {
    let dir = current.parent().unwrap_or_else(|| Path::new("."));
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_supported(path))
        .collect();

    files.sort_by(|a, b| natural_cmp(&file_name(a), &file_name(b)));

    let current = current
        .canonicalize()
        .unwrap_or_else(|_| current.to_path_buf());
    let current_index = files
        .iter()
        .position(|p| {
            p.canonicalize().unwrap_or_else(|_| p.clone()) == current
        })
        .unwrap_or(0);

    DirListing {
        files,
        current_index,
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl DirListing {
    pub fn next(&mut self) -> Option<&Path> {
        if self.files.is_empty() {
            return None;
        }
        self.current_index = (self.current_index + 1) % self.files.len();
        Some(&self.files[self.current_index])
    }

    pub fn prev(&mut self) -> Option<&Path> {
        if self.files.is_empty() {
            return None;
        }
        self.current_index =
            (self.current_index + self.files.len() - 1) % self.files.len();
        Some(&self.files[self.current_index])
    }
}

/// Natural-order comparison so "img2" sorts before "img10".
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();

    loop {
        match (ai.peek(), bi.peek()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(ac), Some(bc)) => {
                if ac.is_ascii_digit() && bc.is_ascii_digit() {
                    let a_num = take_number(&mut ai);
                    let b_num = take_number(&mut bi);
                    match a_num.cmp(&b_num) {
                        std::cmp::Ordering::Equal => continue,
                        other => return other,
                    }
                } else {
                    let (ac, bc) = (ac.to_ascii_lowercase(), bc.to_ascii_lowercase());
                    if ac != bc {
                        return ac.cmp(&bc);
                    }
                    ai.next();
                    bi.next();
                }
            }
        }
    }
}

fn take_number(iter: &mut std::iter::Peekable<std::str::Chars>) -> u64 {
    let mut n: u64 = 0;
    while let Some(c) = iter.peek() {
        if let Some(d) = c.to_digit(10) {
            n = n.saturating_mul(10).saturating_add(d as u64);
            iter.next();
        } else {
            break;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sort_orders_numbers_numerically() {
        let mut names = vec!["img10.jpg", "img2.jpg", "img1.jpg", "IMG3.jpg"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, vec!["img1.jpg", "img2.jpg", "IMG3.jpg", "img10.jpg"]);
    }

    #[test]
    fn is_supported_matches_case_insensitively() {
        assert!(is_supported(Path::new("photo.HEIC")));
        assert!(is_supported(Path::new("photo.WebP")));
        assert!(is_supported(Path::new("anim.APNG")));
        assert!(!is_supported(Path::new("photo.txt")));
    }

    fn listing(len: usize, start: usize) -> DirListing {
        DirListing {
            files: (0..len)
                .map(|i| PathBuf::from(format!("img{i}.png")))
                .collect(),
            current_index: start,
        }
    }

    #[test]
    fn next_wraps_around_to_first() {
        let mut l = listing(3, 2);
        assert_eq!(l.next(), Some(Path::new("img0.png")));
        assert_eq!(l.current_index, 0);
    }

    #[test]
    fn prev_wraps_around_to_last() {
        let mut l = listing(3, 0);
        assert_eq!(l.prev(), Some(Path::new("img2.png")));
        assert_eq!(l.current_index, 2);
    }

    #[test]
    fn next_and_prev_on_empty_listing_return_none() {
        let mut l = listing(0, 0);
        assert_eq!(l.next(), None);
        assert_eq!(l.prev(), None);
    }
}
