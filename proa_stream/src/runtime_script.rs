// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{CspNonce, WriteBuf, WriteError};

macro_rules! runtime_body {
    () => {
        r#"(function(){if(window.$proaBatch)return;var m=new Map(),z=false;function q(r){var w=document.createTreeWalker(r,128);while(w.nextNode()){var c=w.currentNode,v=c.nodeValue||"",k,i,p;if(v.indexOf("proa:start:")===0){k="s";i=v.slice(11)}else if(v.indexOf("proa:end:")===0){k="e";i=v.slice(9)}else continue;p=m.get(i)||{};p[k]=c;m.set(i,p)}}function a(id){var k=String(id),t=document.getElementById("proa-tpl-"+k),p=m.get(k),s=p&&p.s,e=p&&p.e;if(!t||t.tagName!=="TEMPLATE"||t.getAttribute("data-proa-encoding")!=="base64-v1"||!s||!e||!s.parentNode||s.parentNode!==e.parentNode)return;try{var b=atob(t.content.textContent||""),u=new Uint8Array(b.length);for(var i=0;i<b.length;i++)u[i]=b.charCodeAt(i);t.innerHTML=new TextDecoder().decode(u)}catch(_){return}var f=t.content.cloneNode(true);q(f);var n=s.parentNode,x=s.nextSibling;while(x&&x!==e){var y=x.nextSibling;n.removeChild(x);x=y}n.insertBefore(f,e);n.removeChild(s);n.removeChild(e);t.remove();m.delete(k);document.dispatchEvent(new CustomEvent("proa:boundary-ready",{detail:{id:id}}))}window.$proaBatch=function(ids){if(!z){q(document);z=true}for(var i=0;i<ids.length;i++)a(ids[i])};window.$proa=function(id){window.$proaBatch([id])};})();"#
    };
}

const RUNTIME_BODY: &str = runtime_body!();
const RUNTIME: &str = concat!("<script>", runtime_body!(), "</script>");

pub fn inline_runtime_tag() -> &'static str {
    RUNTIME
}

/// Append the out-of-order streaming runtime, applying the request nonce when
/// one is configured. The nonced path writes directly to the destination.
pub fn write_inline_runtime_tag<B: WriteBuf>(
    out: &mut B,
    nonce: Option<&CspNonce>,
) -> Result<(), WriteError> {
    let Some(nonce) = nonce else {
        return out.extend_static(RUNTIME.as_bytes());
    };

    out.extend_static(b"<script")?;
    nonce.write_html_attr(out)?;
    out.extend_static(b">")?;
    out.extend_static(RUNTIME_BODY.as_bytes())?;
    out.extend_static(b"</script>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonced_runtime_preserves_the_existing_script_body() {
        let nonce = CspNonce::new("c3RyZWFtLW5vbmNl").unwrap();
        let mut out = Vec::new();
        write_inline_runtime_tag(&mut out, Some(&nonce)).unwrap();
        let html = String::from_utf8(out).unwrap();
        assert!(html.starts_with(r#"<script nonce="c3RyZWFtLW5vbmNl">"#));
        assert!(html.ends_with("</script>"));
        assert_eq!(
            html.replace(r#" nonce="c3RyZWFtLW5vbmNl""#, ""),
            inline_runtime_tag()
        );
    }

    #[test]
    fn absent_nonce_is_byte_compatible() {
        let mut out = Vec::new();
        write_inline_runtime_tag(&mut out, None).unwrap();
        assert_eq!(out, inline_runtime_tag().as_bytes());
    }

    #[test]
    fn runtime_requires_and_decodes_the_versioned_transport_before_swapping() {
        assert!(RUNTIME_BODY.contains(r#"getAttribute("data-proa-encoding")!=="base64-v1""#));
        assert!(RUNTIME_BODY.contains("atob(t.content.textContent"));
        assert!(RUNTIME_BODY.contains("new TextDecoder().decode(u)"));
        assert!(RUNTIME_BODY.contains("t.innerHTML="));
        assert!(
            RUNTIME_BODY.find("t.innerHTML=").unwrap()
                < RUNTIME_BODY.find("n.insertBefore(").unwrap()
        );
    }

    #[test]
    fn runtime_indexes_comments_once_and_supports_server_batches() {
        assert!(RUNTIME_BODY.contains("window.$proaBatch=function(ids)"));
        assert!(RUNTIME_BODY.contains("if(!z){q(document);z=true}"));
        assert!(RUNTIME_BODY.contains("var m=new Map()"));
        assert!(RUNTIME_BODY.contains("q(f)"));
        assert!(RUNTIME_BODY.contains("window.$proa=function(id)"));
    }
}
