# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

# Build environment for proa_og with WebKitGTK headless rendering.
#
# Uses WebKitGTK (same WebCore layout engine as WPE WebKit) with Xvfb
# virtual framebuffer for fully headless operation. No GPU required.
#
# Usage:
#   docker build -t proa-og-dev -f proa_og/Dockerfile .
#   docker run -it --rm -v $(pwd):/workspace proa-og-dev bash
#   # Inside container:
#   cd /workspace && cargo check -p proa_og
#   xvfb-run cargo test -p proa_og

FROM fedora:41

# WebKitGTK 2.50+ (same WebCore engine as WPE)
RUN dnf install -y \
    webkit2gtk4.1-devel \
    # GLib/GObject/GTK
    gtk4-devel \
    glib2-devel \
    # Virtual framebuffer for headless rendering
    xorg-x11-server-Xvfb \
    # Mesa software rendering
    mesa-dri-drivers \
    mesa-libEGL-devel \
    # Build tools
    gcc gcc-c++ \
    pkg-config \
    cmake \
    clang-devel \
    # Fonts
    google-noto-sans-fonts \
    mozilla-fira-sans-fonts \
    fontconfig \
    # Utils
    curl git \
    && dnf clean all

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
ENV PATH="/root/.cargo/bin:${PATH}"

# Verify WebKitGTK is discoverable
RUN pkg-config --modversion webkit2gtk-4.1

# Convenience: wrapper script to run commands under Xvfb
RUN echo '#!/bin/bash\nexec xvfb-run -a -s "-screen 0 1280x720x24" "$@"' > /usr/local/bin/headless && \
    chmod +x /usr/local/bin/headless

WORKDIR /workspace
CMD ["bash"]
