// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(wpe_stub)");

    let use_wpe = env::var("CARGO_FEATURE_WPE").is_ok();

    if use_wpe {
        build_wpe();
    } else {
        build_webkitgtk();
    }
}

/// Build with WPE WebKit (no GTK, no display server required).
fn build_wpe() {
    // Try wpe-webkit-2.0 first (newer), then wpe-webkit-1.1 (older)
    let webkit = pkg_config::Config::new()
        .atleast_version("2.38")
        .probe("wpe-webkit-2.0")
        .or_else(|_| {
            pkg_config::Config::new()
                .atleast_version("2.28")
                .probe("wpe-webkit-1.1")
        });

    let wpe = pkg_config::Config::new().probe("wpe-1.0").ok();
    let cairo = pkg_config::Config::new().probe("cairo").ok();

    match webkit {
        Ok(lib) => {
            let mut builder = bindgen::Builder::default()
                .header("src/wrapper.h")
                .clang_arg("-DUSE_WPE")
                // WebKit functions
                .allowlist_function("webkit_.*")
                // GObject/GLib
                .allowlist_function("g_object_new")
                .allowlist_function("g_object_unref")
                .allowlist_function("g_object_set")
                .allowlist_function("g_signal_connect_data")
                .allowlist_function("g_signal_handler_disconnect")
                .allowlist_function("g_main_loop_.*")
                .allowlist_function("g_main_context_.*")
                // Render-timeout watchdog (g_timeout_add_seconds + g_source_remove)
                .allowlist_function("g_timeout_add_seconds")
                .allowlist_function("g_source_remove")
                .allowlist_function("g_error_free")
                // Cairo (for snapshot surface)
                .allowlist_function("cairo_surface_.*")
                .allowlist_function("cairo_image_surface_.*")
                .allowlist_function("cairo_create")
                .allowlist_function("cairo_destroy")
                .allowlist_function("cairo_set_source_surface")
                .allowlist_function("cairo_paint")
                .allowlist_function("cairo_format_stride_for_width")
                // Types
                .allowlist_type("WebKit.*")
                .allowlist_type("cairo_.*")
                .allowlist_type("GMainLoop")
                .allowlist_type("GAsyncResult")
                .allowlist_type("GError")
                .allowlist_var("WEBKIT_.*")
                .allowlist_var("CAIRO_.*")
                .derive_debug(true)
                .derive_default(true)
                .opaque_type("_GObject")
                .opaque_type("_GObjectClass")
                .opaque_type("_GObjectConstructParam");

            for path in &lib.include_paths {
                builder = builder.clang_arg(format!("-I{}", path.display()));
            }
            if let Some(ref w) = wpe {
                for path in &w.include_paths {
                    builder = builder.clang_arg(format!("-I{}", path.display()));
                }
            }
            if let Some(ref c) = cairo {
                for path in &c.include_paths {
                    builder = builder.clang_arg(format!("-I{}", path.display()));
                }
            }

            let bindings = builder
                .generate()
                .expect("Failed to generate WPE WebKit bindings");

            let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
            bindings
                .write_to_file(out_path.join("wpe_bindings.rs"))
                .expect("Failed to write bindings");
        }
        Err(e) => {
            eprintln!("cargo:warning=WPE WebKit not found: {e}");
            eprintln!("cargo:warning=Building with stub bindings. Install libwpewebkit-2.0-dev and libwpe-1.0-dev.");

            let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
            std::fs::write(
                out_path.join("wpe_bindings.rs"),
                "// Stub bindings — WPE WebKit not available on this platform\n",
            )
            .expect("Failed to write stub bindings");

            println!("cargo:rustc-cfg=wpe_stub");
        }
    }
}

/// Build with WebKitGTK (requires GTK3 + display server or Xvfb).
fn build_webkitgtk() {
    // Try WebKitGTK first (widely available), then WPE
    let webkit = pkg_config::Config::new()
        .atleast_version("2.40")
        .probe("webkit2gtk-4.1")
        .or_else(|_| {
            pkg_config::Config::new()
                .atleast_version("2.38")
                .probe("webkit2gtk-4.0")
        });

    let gtk = pkg_config::Config::new().probe("gtk+-3.0").ok();
    let cairo = pkg_config::Config::new().probe("cairo").ok();

    match webkit {
        Ok(lib) => {
            let mut builder = bindgen::Builder::default()
                .header("src/wrapper.h")
                // WebKit functions
                .allowlist_function("webkit_.*")
                // GTK functions we need
                .allowlist_function("gtk_init")
                .allowlist_function("gtk_init_check")
                .allowlist_function("gtk_offscreen_window_new")
                .allowlist_function("gtk_container_add")
                .allowlist_function("gtk_widget_.*")
                .allowlist_function("gtk_window_.*")
                // GObject/GLib
                .allowlist_function("g_object_new")
                .allowlist_function("g_object_unref")
                .allowlist_function("g_object_set")
                .allowlist_function("g_signal_connect_data")
                .allowlist_function("g_signal_handler_disconnect")
                .allowlist_function("g_main_loop_.*")
                .allowlist_function("g_main_context_.*")
                // Render-timeout watchdog (g_timeout_add_seconds + g_source_remove)
                .allowlist_function("g_timeout_add_seconds")
                .allowlist_function("g_source_remove")
                .allowlist_function("g_error_free")
                // Cairo (for snapshot surface)
                .allowlist_function("cairo_surface_.*")
                .allowlist_function("cairo_image_surface_.*")
                .allowlist_function("cairo_create")
                .allowlist_function("cairo_destroy")
                .allowlist_function("cairo_set_source_surface")
                .allowlist_function("cairo_paint")
                .allowlist_function("cairo_format_stride_for_width")
                // Types
                .allowlist_type("WebKit.*")
                .allowlist_type("Gtk.*")
                .allowlist_type("cairo_.*")
                .allowlist_type("GMainLoop")
                .allowlist_type("GAsyncResult")
                .allowlist_type("GError")
                .allowlist_var("WEBKIT_.*")
                .allowlist_var("GTK_.*")
                .allowlist_var("CAIRO_.*")
                .derive_debug(true)
                .derive_default(true)
                // Make complex GObject/GTK structs opaque to avoid
                // pulling in their entire field definitions
                .opaque_type("_GtkWidget")
                .opaque_type("_GtkWidgetPrivate")
                .opaque_type("_GtkWidgetPath")
                .opaque_type("_GtkWidgetClass")
                .opaque_type("_GObject")
                .opaque_type("_GObjectClass")
                .opaque_type("_GObjectConstructParam");

            for path in &lib.include_paths {
                builder = builder.clang_arg(format!("-I{}", path.display()));
            }
            if let Some(ref g) = gtk {
                for path in &g.include_paths {
                    builder = builder.clang_arg(format!("-I{}", path.display()));
                }
            }
            if let Some(ref c) = cairo {
                for path in &c.include_paths {
                    builder = builder.clang_arg(format!("-I{}", path.display()));
                }
            }

            let bindings = builder
                .generate()
                .expect("Failed to generate WebKitGTK bindings");

            let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
            bindings
                .write_to_file(out_path.join("wpe_bindings.rs"))
                .expect("Failed to write bindings");
        }
        Err(e) => {
            eprintln!("cargo:warning=WebKitGTK not found: {e}");
            eprintln!("cargo:warning=Building with stub bindings. Use Docker for full builds.");

            let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
            std::fs::write(
                out_path.join("wpe_bindings.rs"),
                "// Stub bindings — WebKitGTK not available on this platform\n",
            )
            .expect("Failed to write stub bindings");

            println!("cargo:rustc-cfg=wpe_stub");
        }
    }
}
