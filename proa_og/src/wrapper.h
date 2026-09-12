/* Copyright Proa Labs, Inc. */
/* SPDX-License-Identifier: Apache-2.0 */

// Wrapper header for bindgen — includes the WebKit headless rendering API.
// When USE_WPE is defined, uses WPE WebKit (no GTK dependency).
// Otherwise uses WebKitGTK (requires GTK3 + display server or Xvfb).
#ifdef USE_WPE
#include <wpe/webkit.h>
#else
#include <webkit2/webkit2.h>
#include <gtk/gtk.h>
#endif
#include <cairo/cairo.h>
