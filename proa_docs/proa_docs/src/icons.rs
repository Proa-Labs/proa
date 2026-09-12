// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub fn nav_icon_svg(icon: &str) -> &'static str {
    match icon {
        "ArrowRightLeft" => ARROW_RIGHT_LEFT_ICON,
        "BookOpen" => BOOK_OPEN_ICON,
        "Braces" => BRACES_ICON,
        "Bot" => BOT_ICON,
        "Brain" => BRAIN_ICON,
        "Cpu" => CPU_ICON,
        "Database" => DATABASE_ICON,
        "FileText" => FILE_TEXT_ICON,
        "Layers" => LAYERS_ICON,
        "LifeBuoy" => LIFE_BUOY_ICON,
        "Palette" => PALETTE_ICON,
        "Rocket" => ROCKET_ICON,
        "Route" => ROUTE_ICON,
        "Settings" => SETTINGS_ICON,
        "ShieldCheck" => SHIELD_CHECK_ICON,
        "Terminal" => TERMINAL_ICON,
        "Zap" => ZAP_ICON,
        _ => FILE_TEXT_ICON,
    }
}

macro_rules! nav_icon {
    ($body:literal) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
            $body,
            "</svg>"
        )
    };
}

const ARROW_RIGHT_LEFT_ICON: &str = nav_icon!(
    r#"<path d="m16 3 4 4-4 4"/><path d="M20 7H4"/><path d="m8 21-4-4 4-4"/><path d="M4 17h16"/>"#
);
const BOOK_OPEN_ICON: &str = nav_icon!(
    r#"<path d="M12 7v14"/><path d="M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z"/>"#
);
const BRACES_ICON: &str = nav_icon!(
    r#"<path d="M8 3H7a2 2 0 0 0-2 2v4a2 2 0 0 1-2 2 2 2 0 0 1 2 2v4a2 2 0 0 0 2 2h1"/><path d="M16 3h1a2 2 0 0 1 2 2v4a2 2 0 0 0 2 2 2 2 0 0 0-2 2v4a2 2 0 0 1-2 2h-1"/>"#
);
const BRAIN_ICON: &str = nav_icon!(
    r#"<path d="M12 18V5"/><path d="M15 13a4.17 4.17 0 0 1-3-4 4.17 4.17 0 0 1-3 4"/><path d="M18 18a4 4 0 0 0 2-7.46"/><path d="M6 18a4 4 0 0 1-2-7.46"/><path d="M18 5.13a4 4 0 0 1 2.52 5.77"/><path d="M6 5.13a4 4 0 0 0-2.52 5.77"/><path d="M18 6.5A3 3 0 1 0 12 5a3 3 0 1 0-6 1.5"/>"#
);
const CPU_ICON: &str = nav_icon!(
    r#"<rect width="16" height="16" x="4" y="4" rx="2"/><rect width="6" height="6" x="9" y="9" rx="1"/><path d="M15 2v2"/><path d="M15 20v2"/><path d="M2 15h2"/><path d="M2 9h2"/><path d="M20 15h2"/><path d="M20 9h2"/><path d="M9 2v2"/><path d="M9 20v2"/>"#
);
const FILE_TEXT_ICON: &str = nav_icon!(
    r#"<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M10 9H8"/><path d="M16 13H8"/><path d="M16 17H8"/>"#
);
const LAYERS_ICON: &str = nav_icon!(
    r#"<path d="m12.83 2.18 7.34 4.25a1 1 0 0 1 0 1.74l-7.34 4.25a1.66 1.66 0 0 1-1.66 0L3.83 8.17a1 1 0 0 1 0-1.74l7.34-4.25a1.66 1.66 0 0 1 1.66 0z"/><path d="m22 12.5-9.17 5.32a1.66 1.66 0 0 1-1.66 0L2 12.5"/><path d="m22 17.5-9.17 5.32a1.66 1.66 0 0 1-1.66 0L2 17.5"/>"#
);
const ROUTE_ICON: &str = nav_icon!(
    r#"<circle cx="6" cy="19" r="3"/><path d="M9 19h8.5a3.5 3.5 0 0 0 0-7h-11a3.5 3.5 0 0 1 0-7H15"/><circle cx="18" cy="5" r="3"/>"#
);
const SETTINGS_ICON: &str = nav_icon!(
    r#"<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.38a2 2 0 0 0-.73-2.73l-.15-.09a2 2 0 0 1-1-1.74v-.51a2 2 0 0 1 1-1.72l.15-.1a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>"#
);
const TERMINAL_ICON: &str = nav_icon!(r#"<path d="m4 17 6-6-6-6"/><path d="M12 19h8"/>"#);
const BOT_ICON: &str = nav_icon!(
    r#"<path d="M12 8V4H8"/><rect width="16" height="12" x="4" y="8" rx="2"/><path d="M2 14h2"/><path d="M20 14h2"/><path d="M15 13v2"/><path d="M9 13v2"/>"#
);
const DATABASE_ICON: &str = nav_icon!(
    r#"<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M3 5V19A9 3 0 0 0 21 19V5"/><path d="M3 12A9 3 0 0 0 21 12"/>"#
);
const LIFE_BUOY_ICON: &str = nav_icon!(
    r#"<circle cx="12" cy="12" r="10"/><path d="m4.93 4.93 4.24 4.24"/><path d="m14.83 9.17 4.24-4.24"/><path d="m14.83 14.83 4.24 4.24"/><path d="m9.17 14.83-4.24 4.24"/><circle cx="12" cy="12" r="4"/>"#
);
const PALETTE_ICON: &str = nav_icon!(
    r#"<path d="M12 22a1 1 0 0 1 0-20 10 9 0 0 1 10 9 5 5 0 0 1-5 5h-2.25a1.75 1.75 0 0 0-1.4 2.8l.3.4a1.75 1.75 0 0 1-1.4 2.8z"/><circle cx="13.5" cy="6.5" r=".5" fill="currentColor"/><circle cx="17.5" cy="10.5" r=".5" fill="currentColor"/><circle cx="6.5" cy="12.5" r=".5" fill="currentColor"/><circle cx="8.5" cy="7.5" r=".5" fill="currentColor"/>"#
);
const ROCKET_ICON: &str = nav_icon!(
    r#"<path d="M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91 0z"/><path d="m12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z"/><path d="M9 12H4s.55-3.03 2-4c1.62-1.08 5 0 5 0"/><path d="M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5"/>"#
);
const SHIELD_CHECK_ICON: &str = nav_icon!(
    r#"<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/><path d="m9 12 2 2 4-4"/>"#
);
const ZAP_ICON: &str = nav_icon!(
    r#"<path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z"/>"#
);
