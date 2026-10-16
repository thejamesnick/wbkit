//! Human-readable output: sizes, times, errors and the `info` listing.

use std::time::{SystemTime, UNIX_EPOCH};
use wbkit_core::wbx::{Package, WbxError};

pub fn size(bytes: usize) -> String {
    match bytes {
        b if b >= 1024 * 1024 => format!("{:.2} MB", b as f64 / 1048576.0),
        b if b >= 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{} B", b),
    }
}

/// Explains the limits in upload terms, and keeps the stable code for scripts and support.
pub fn explain(error: WbxError) -> String {
    let hint = match error {
        WbxError::TooManyFiles => " WebBox uploads accept up to 512 files.",
        WbxError::TooBig => " WebBox uploads accept packages up to 50 MB, files up to 50 MB and 200 MB unpacked.",
        _ => "",
    };
    format!("{}{} [{}]", error, hint, error.code())
}

/// SOURCE_DATE_EPOCH (for reproducible builds) when set, otherwise the current time, as an ISO UTC string.
pub fn now_or_source_date() -> String {
    let seconds = std::env::var("SOURCE_DATE_EPOCH").ok().and_then(|v| v.parse::<i64>().ok())
        .unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0));
    let (days, rest) = (seconds.div_euclid(86400), seconds.rem_euclid(86400));
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.000Z", year, month, day, rest / 3600, rest % 3600 / 60, rest % 60)
}

pub fn info(package: &Package, package_bytes: usize) {
    let m = &package.header.manifest;
    let yes = |on: bool| if on { "yes" } else { "no" };
    println!("{} {}  ({})", m.name, m.version, m.id);
    println!("by {}  ·  {}  ·  rated {}", m.author, m.category, m.rating);
    if !m.description.is_empty() { println!("\n{}", m.description); }
    println!();
    match &m.url {
        Some(url) => println!("Hosted at     {}", url),
        None => println!("Starts at     {}", m.entry_point),
    }
    println!("Permissions   {}", if m.webbox.permissions.is_empty() { "none".to_string() } else { m.webbox.permissions.join(", ") });
    println!("Display       {} {}x{} {}, {}, full screen: {}", m.technical.renderer, m.technical.resolution.width, m.technical.resolution.height, m.technical.resolution.aspect_ratio, m.technical.orientation, yes(m.technical.fullscreen));
    let f = &m.webbox.features;
    println!("Features      multiplayer {}, achievements {}, leaderboards {}, cloud save {}", yes(f.multiplayer), yes(f.achievements), yes(f.leaderboards), yes(f.cloud_save));
    if !m.tags.is_empty() { println!("Tags          {}", m.tags.join(", ")); }
    if let Some(icon) = &m.icon { println!("Cover         {}", icon); }
    println!("\nFormat {}, built {}, {} packed, {} files:", package.header.version, package.header.created, size(package_bytes), package.files.len());
    for (path, data) in &package.files { println!("  {:>10}  {}", size(data.len()), path); }
}
