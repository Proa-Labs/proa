// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_macros::{html_sync, xml, xml_sync};

fn main() {
    let _ = xml_sync! { <?xml encoding="UTF-8"?> <Feed/> };
    let _ = xml_sync! { <?xml version="1.1"?> <Feed/> };
    let _ = xml! { <?xml version="1.0" encoding="UTF-16"?> <Feed/> };
    let _ = xml_sync! { <?xml version="1.0" standalone="yes" encoding="UTF-8"?> <Feed/> };
    let _ = xml_sync! { <?xml version="1.0" version="1.0"?> <Feed/> };
    let _ = xml_sync! { <?xml version="1.0" standalone="true"?> <Feed/> };
    let encoding = "UTF-8";
    let _ = xml_sync! { <?xml version="1.0" encoding={encoding}?> <Feed/> };
    let _ = xml_sync! { <Feed><?xml version="1.0"?></Feed> };
    let _ = xml_sync! { <?xml version="1.0"?> <?xml version="1.0"?> <Feed/> };
    let _ = xml_sync! { "\n" <?xml version="1.0"?> <Feed/> };
    let _ = html_sync! { <?xml version="1.0"?> <div/> };
    let _ = xml_sync! { <?stylesheet href="style.xsl"?> <Feed/> };
}
