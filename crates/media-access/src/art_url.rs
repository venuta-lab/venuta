//! Asking the catalog's picture host for a cover at the size it is shown.
//!
//! Catalog cover addresses carry their size inside them: `…=w60-h60-l90-rj` on album art,
//! `…=s120-c-k…` on artist pictures. A list hands out small ones, and drawing a 120 px picture
//! in a 400 px box is what made online covers look soft. The host serves any size asked for, so
//! the number is changed to the size wanted. Addresses whose size is signed into them (video
//! frames with `sqp=`) and any other host are left as they are.

/// The address of the same picture at `px` pixels square (rounded up to a size the host keeps
/// cached). Unchanged when the address has no size to change.
pub fn sized_art(url: &str, px: u32) -> String {
    let px = bucket(px);
    let host = url.split('/').nth(2).unwrap_or_default();
    let resizable = host.ends_with("googleusercontent.com") || host.ends_with("ggpht.com");
    if !resizable {
        return url.to_owned();
    }
    // The size options sit after the last '=' of the path (no query string on these hosts).
    let Some(eq) = url.rfind('=') else {
        return url.to_owned();
    };
    let (base, options) = url.split_at(eq + 1);
    if options.contains('/') {
        return url.to_owned();
    }
    let mut changed = false;
    let parts: Vec<String> = options
        .split('-')
        .map(|part| {
            let size_of = |prefix: char| {
                part.strip_prefix(prefix)
                    .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
                    .is_some()
            };
            if size_of('w') || size_of('h') || size_of('s') {
                changed = true;
                format!("{}{px}", &part[..1])
            } else {
                part.to_owned()
            }
        })
        .collect();
    if !changed {
        return url.to_owned();
    }
    format!("{base}{}", parts.join("-"))
}

/// Sizes asked for, so the same picture at nearby sizes is one cached file.
fn bucket(px: u32) -> u32 {
    const SIZES: [u32; 6] = [120, 226, 360, 544, 800, 1200];
    SIZES.into_iter().find(|s| *s >= px).unwrap_or(1200)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_art_is_asked_for_at_the_size_shown() {
        let url = "https://lh3.googleusercontent.com/abc=w60-h60-l90-rj";
        assert_eq!(
            sized_art(url, 544),
            "https://lh3.googleusercontent.com/abc=w544-h544-l90-rj"
        );
    }

    #[test]
    fn an_artist_picture_keeps_its_crop_options() {
        let url = "https://yt3.ggpht.com/xyz=s120-c-k-c0x00ffffff-no-rj";
        assert_eq!(
            sized_art(url, 300),
            "https://yt3.ggpht.com/xyz=s360-c-k-c0x00ffffff-no-rj"
        );
    }

    #[test]
    fn sizes_round_up_to_a_few_cached_ones_and_stop_at_1200() {
        let url = "https://lh3.googleusercontent.com/abc=w60-h60";
        assert_eq!(
            sized_art(url, 100),
            "https://lh3.googleusercontent.com/abc=w120-h120"
        );
        assert_eq!(
            sized_art(url, 5000),
            "https://lh3.googleusercontent.com/abc=w1200-h1200"
        );
    }

    #[test]
    fn video_frames_and_other_hosts_are_left_alone() {
        let frame = "https://i.ytimg.com/vi/abc/hqdefault.jpg?sqp=-oaymwEW&rs=AOn4";
        assert_eq!(sized_art(frame, 544), frame);
        let other = "https://example.com/cover=w60-h60";
        assert_eq!(sized_art(other, 544), other);
        let no_size = "https://lh3.googleusercontent.com/abc";
        assert_eq!(sized_art(no_size, 544), no_size);
    }
}
