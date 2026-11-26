# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

# Cargo logs the fingerprint it checks for each unit, including cache hits,
# before it decides whether to run rustc. Keep the usual build diagnostics.
/ DEBUG .*cargo::(core::)?compiler::fingerprint: / {
    if (sub(/^.*cargo::(core::)?compiler::fingerprint: fingerprint at: /, "")) {
        print >> fingerprints
    }
    next
}
{ print; fflush() }
