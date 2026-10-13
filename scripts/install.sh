#!/bin/sh
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

# Proa CLI installer for Linux and macOS.
#
#   curl -fsSL https://proa.so/install.sh | sh
#
# This file is designed to be piped into sh, where the shell executes whatever
# it has parsed so far. Every statement therefore lives inside a function and
# `main "$@"` is the very last line, so a connection that dies mid-transfer
# leaves a script that defines functions and does nothing, rather than one that
# has already half-installed something.
#
# POSIX sh only. It has to run under dash (Debian/Ubuntu /bin/sh), busybox ash
# (Alpine), bash and zsh, so there are no arrays, no [[ ]], no `function`, and
# no pipefail. `local` is used because every one of those shells has it.
#
# Every run also writes a log file (see the Logging section below). This script
# runs on machines nobody will ever see, so when it fails the only evidence is
# a scrollback that is usually already gone. The log is what a user can attach
# to a bug report, and it deliberately records the detail the console does not
# show: every URL, every checksum, every fallback and why it was taken.
#
# shellcheck shell=sh
# shellcheck disable=SC3043
# SC3043 fires on every `local`. It is deliberate: `local` is outside POSIX but
# dash, busybox ash, bash, zsh and ksh93 all implement it, and those are the
# only shells this installer supports. Dropping it would leak every helper
# variable into the global scope of a script that runs in the user's shell.

set -eu

# ---------------------------------------------------------------------------
# Configuration and small helpers
# ---------------------------------------------------------------------------

set_defaults() {
    # HOME drives the default install root and every profile path, so an unset
    # HOME is worth catching here rather than producing a path like "/.proa".
    if [ -z "${HOME:-}" ]; then
        printf 'error: HOME is not set, cannot pick an install directory\n' >&2
        exit 1
    fi

    # Minimum supported Rust version, kept in sync with [workspace.package]
    # rust-version in the repository root Cargo.toml.
    MSRV="1.93.0"

    # Bump this when the installer itself changes. It is the only thing in a
    # pasted log that identifies which installer produced it, since the script
    # is served from a URL and carries no other version.
    INSTALLER_REVISION="2026-10-12"

    # Tailwind is an optional companion install. Keep the version and hashes
    # pinned together: changing the version without updating every hash below
    # must make verification fail rather than silently trusting new bytes.
    TAILWIND_VERSION="4.3.3"
    TAILWIND_RELEASE_BASE="https://github.com/tailwindlabs/tailwindcss/releases/download/v$TAILWIND_VERSION"

    # Two settings that are easy to confuse, so they are spelled out here as
    # well as in --help:
    #
    #   DOWNLOAD_BASE  where prebuilt archives and the version lookup come from.
    #   REPO           the git remote for snapshots and explicit Git builds.
    #                  Official released versions build from crates.io.
    #
    # They used to be the same thing, back when this script fetched archives
    # straight off GitHub Releases. It no longer does. proa.so sits in front of
    # those releases, so a download never needs repository access, and REPO now
    # only reaches 'cargo install --git'. Overriding one does not move the
    # other.
    DOWNLOAD_BASE="${PROA_DOWNLOAD_BASE:-https://proa.so/downloads/cli}"
    REPO="${PROA_REPO:-Proa-Labs/proa}"
    VERSION="${PROA_VERSION:-}"

    PROA_ROOT="${PROA_HOME:-$HOME/.proa}"
    INSTALL_DIR="${PROA_INSTALL_DIR:-}"
    NO_MODIFY_PATH="${PROA_NO_MODIFY_PATH:-}"
    BUILD_FROM_SOURCE="${PROA_BUILD_FROM_SOURCE:-}"
    PREBUILT_ONLY="${PROA_PREBUILT_ONLY:-}"
    TAILWIND_CHOICE="${PROA_TAILWIND:-ask}"
    TAILWIND_ONLY=""
    ASSUME_YES="${PROA_YES:-}"
    VERBOSE="${PROA_VERBOSE:-}"

    # Where each setting came from, for the log. A bug report where the flag
    # and the environment disagree is otherwise impossible to read.
    SRC_VERSION="default"
    SRC_HOME="default"
    SRC_INSTALL_DIR="default"
    SRC_DOWNLOAD_BASE="default"
    SRC_REPO="default"
    SRC_NO_MODIFY_PATH="default"
    SRC_BUILD_FROM_SOURCE="default"
    SRC_PREBUILT_ONLY="default"
    SRC_TAILWIND="default"
    SRC_TAILWIND_ONLY="default"
    SRC_YES="default"
    SRC_VERBOSE="default"
    SRC_NO_COLOR="unset"

    if [ -n "${PROA_VERSION:-}" ]; then SRC_VERSION="environment"; fi
    if [ -n "${PROA_HOME:-}" ]; then SRC_HOME="environment"; fi
    if [ -n "${PROA_INSTALL_DIR:-}" ]; then SRC_INSTALL_DIR="environment"; fi
    if [ -n "${PROA_DOWNLOAD_BASE:-}" ]; then SRC_DOWNLOAD_BASE="environment"; fi
    if [ -n "${PROA_REPO:-}" ]; then SRC_REPO="environment"; fi
    if [ -n "${PROA_NO_MODIFY_PATH:-}" ]; then SRC_NO_MODIFY_PATH="environment"; fi
    if [ -n "${PROA_BUILD_FROM_SOURCE:-}" ]; then SRC_BUILD_FROM_SOURCE="environment"; fi
    if [ -n "${PROA_PREBUILT_ONLY:-}" ]; then SRC_PREBUILT_ONLY="environment"; fi
    if [ -n "${PROA_TAILWIND:-}" ]; then SRC_TAILWIND="environment"; fi
    if [ -n "${PROA_YES:-}" ]; then SRC_YES="environment"; fi
    if [ -n "${PROA_VERBOSE:-}" ]; then SRC_VERBOSE="environment"; fi
    if [ -n "${NO_COLOR:-}" ]; then SRC_NO_COLOR="environment"; fi

    # Set when the user names a directory explicitly, which changes what the
    # source fallback has to warn about.
    DIR_WAS_EXPLICIT=""
    TAILWIND_FLAG=""

    OS_NAME=""
    ARCH_NAME=""
    UNAME_S=""
    UNAME_M=""
    LIBC_KIND=""
    LIBC_EVIDENCE=""
    TARGET=""
    TARGET_REASON=""
    RUST_STATE=""
    RUST_VERSION=""
    DOWNLOADER=""
    WGET_STRICT_TLS=""
    WGET_PROGRESS=""
    TMP_DIR=""
    BIN_DIR=""
    PREVIOUS_VERSION=""
    SHA_TOOL=""

    # Logging state. LOG_ENABLED is the single switch every log write checks,
    # so a log that cannot be created degrades to console-only rather than
    # taking the install down with it.
    LOG_FILE=""
    DEFAULT_LOG_FILE=""
    LOG_ENABLED=""
    LOG_SOURCE="default"
    LOG_SECTION=""
    LOG_HAVE_DATE=""
    LOG_FOOTER_DONE=""
    LOG_HINT_DONE=""
    HAVE_SECONDS=""
    START_EPOCH=""
    EXIT_STATUS=0

    # Console state. Each of these is decided once and read by the printers in
    # the "Console output" section, so no business logic below ever has to know
    # how wide a column is or which glyph this terminal can render.
    IS_TTY=""
    UTF8_GLYPHS=""
    GLYPH_STEP=""
    GLYPH_OK=""
    GLYPH_FAIL=""

    # Label column widths. See the Console output section for the layout these
    # produce; they are the only numbers the alignment depends on.
    FACT_WIDTH=9
    STEP_WIDTH=12
    LINK_WIDTH=4

    FACT_TOOLCHAIN=""
    INSTALL_MODE=""
    MODE_NOTE=""
    PATH_STATE=""
    PATH_EXPORT_LINE=""
    READY_LINE=""
    LAST_DOWNLOAD_BYTES=""
    CHECKSUM_SIZE_SUFFIX=""
    TAILWIND_ASSET=""
    TAILWIND_SHA256=""
    FAILURE_SHOWN=""

    BOLD=""
    DIM=""
    RED=""
    GREEN=""
    RESET=""

    # ESC is needed whether or not colour is on: the log strips escapes out of
    # anything it is handed, and colour codes in a pasted log file are noise.
    ESC=$(printf '\033')

    # Command substitution eats trailing newlines, so a newline has to be
    # smuggled out behind a sentinel character.
    NL=$(printf '\nx')
    NL="${NL%x}"
}

setup_colors() {
    # IS_TTY is asked twice: once for colour and once for curl's progress bar.
    # Both are properties of the terminal we are writing to rather than of the
    # run, and a redirected install has to stay readable as plain text.
    if [ -t 1 ]; then
        IS_TTY="yes"
    fi

    # NO_COLOR is the cross-tool opt out. https://no-color.org
    #
    # The palette is deliberately this short: bold for the two headlines, dim
    # for label columns and for trailing secondary lines, green for a step that
    # succeeded and red for one that did not. Nothing else, and no background
    # colours, because output that has to survive a hundred terminal themes can
    # only rely on emphasis and not on any particular hue being legible.
    if [ -n "$IS_TTY" ] && [ -z "${NO_COLOR:-}" ]; then
        BOLD=$(printf '\033[1m')
        DIM=$(printf '\033[2m')
        RED=$(printf '\033[31m')
        GREEN=$(printf '\033[32m')
        RESET=$(printf '\033[0m')
    fi
}

# Picks the step glyphs. UTF-8 is a property of the locale, not of the
# terminal being interactive, so this is decided from LC_ALL, then LC_CTYPE,
# then LANG, in the order the locale rules themselves resolve those variables.
#
# The fallback is not a nicety. A box-drawing character that arrives as
# mojibake on a CI runner is worse than a plain ASCII arrow, so anything this
# cannot positively identify as UTF-8 gets ASCII.
#
# The ASCII forms are two columns wide and the UTF-8 forms are one, which is
# why GLYPH_FAIL carries a trailing space: within one run every glyph is the
# same width, so the label column that follows it always starts in the same
# place.
setup_glyphs() {
    local loc=""

    GLYPH_STEP="->"
    GLYPH_OK="ok"
    GLYPH_FAIL="x "

    if [ -n "${LC_ALL:-}" ]; then
        loc="$LC_ALL"
    elif [ -n "${LC_CTYPE:-}" ]; then
        loc="$LC_CTYPE"
    elif [ -n "${LANG:-}" ]; then
        loc="$LANG"
    fi

    # Case-insensitive without tr: a bracket per letter matches every spelling
    # of "UTF-8" and "utf8" that a locale name uses, and needs no extra tool.
    case "$loc" in
        *[Uu][Tt][Ff]-8* | *[Uu][Tt][Ff]8*)
            # Built from octal escapes rather than written as literals. This
            # script is served over HTTP and piped straight into sh, and the
            # escapes are plain ASCII bytes that no re-encoding proxy or
            # eight-bit-unclean transfer can turn into something else.
            GLYPH_STEP=$(printf '\342\226\270') # U+25B8 right-pointing triangle
            GLYPH_OK=$(printf '\342\234\223')   # U+2713 check mark
            GLYPH_FAIL=$(printf '\342\234\227') # U+2717 ballot x
            UTF8_GLYPHS="yes"
            ;;
    esac
}

# ---------------------------------------------------------------------------
# Logging
# ---------------------------------------------------------------------------
#
# Console output is unchanged by this section: the normal run is exactly as
# quiet as it was. The log gets everything, plus the detail the console
# deliberately omits. --verbose mirrors that detail to the terminal too.
#
# SECURITY: nothing here dumps the environment. Only the specific PROA_* and
# NO_COLOR names documented in --help are recorded, by name. Users really do
# export things like CARGO_REGISTRY_TOKEN, and a file people are asked
# to paste into a public issue must never carry a credential.

# Prints an ISO-8601 UTC timestamp, or nothing when this machine has no usable
# date. A missing date degrades the log, it never fails the install.
utc_now() {
    if [ -n "$LOG_HAVE_DATE" ]; then
        date -u +%Y-%m-%dT%H:%M:%SZ 2> /dev/null || printf ''
    fi
}

# Everything before the first newline of $1.
first_line() {
    case "$1" in
        *"$NL"*) printf '%s' "${1%%"$NL"*}" ;;
        *) printf '%s' "$1" ;;
    esac
}

# Removes CSI colour sequences. Pure parameter expansion, because sed is not
# guaranteed and a log helper must never be the thing that needs a new tool.
strip_ansi() {
    local rest="$1"
    local out=""
    local tail

    while :; do
        case "$rest" in
            *"$ESC"*)
                out="$out${rest%%"$ESC"*}"
                tail="${rest#*"$ESC"}"
                # Every escape this script emits is a CSI sequence ending in m.
                case "$tail" in
                    *m*) rest="${tail#*m}" ;;
                    *) rest="" ;;
                esac
                ;;
            *)
                out="$out$rest"
                break
                ;;
        esac
    done

    printf '%s' "$out"
}

# Replaces the userinfo of a URL with <redacted>, so "https://user:tok@host/p"
# logs as "https://<redacted>@host/p".
#
# A registry index is one of the few values in this script that can arrive from
# the environment carrying a secret: putting a token in the URL is a documented
# way to reach a private cargo registry. The log is a file people are asked to
# paste into a public issue, so it gets the host and the path (which is what
# makes the line worth having) and none of the credential.
#
# Only the authority is searched for the '@', because a path may legitimately
# contain one and stripping to the last '@' in the whole URL would eat it.
redact_url_userinfo() {
    local url="$1"
    local scheme
    local after
    local authority
    local rest

    case "$url" in
        *://*) ;;
        *)
            printf '%s' "$url"
            return 0
            ;;
    esac

    scheme="${url%%://*}"
    after="${url#*://}"
    authority="${after%%/*}"
    rest="${after#"$authority"}"

    case "$authority" in
        *@*) printf '%s://<redacted>@%s%s' "$scheme" "${authority##*@}" "$rest" ;;
        *) printf '%s' "$url" ;;
    esac
}

# $1 level, rest message. Every line in the file is "<timestamp> <LEVEL> <text>",
# so a message that spans lines (the output of `rustc --version` on a broken
# toolchain, say) becomes one prefixed entry per line rather than a first line
# with anonymous continuations.
log_write() {
    local level="$1"
    shift
    local message="$*"

    [ -n "$LOG_ENABLED" ] || return 0

    case "$message" in
        *"$ESC"*) message=$(strip_ansi "$message") ;;
    esac

    case "$message" in
        *"$NL"*)
            while [ -n "$message" ]; do
                log_write_line "$level" "${message%%"$NL"*}"
                case "$message" in
                    *"$NL"*) message="${message#*"$NL"}" ;;
                    *) message="" ;;
                esac
            done
            ;;
        *)
            log_write_line "$level" "$message"
            ;;
    esac
}

# The one place that touches the log file.
log_write_line() {
    local stamp

    stamp=$(utc_now)
    if [ -n "$stamp" ]; then
        printf '%s %s %s\n' "$stamp" "$1" "$2" >> "$LOG_FILE" 2> /dev/null ||
            log_write_failed
    else
        printf '%s %s\n' "$1" "$2" >> "$LOG_FILE" 2> /dev/null ||
            log_write_failed
    fi
}

# A full disk or a vanished directory disables the log once and lets the
# install carry on. LOG_ENABLED is cleared first so the warn below, which
# mirrors into the log, cannot recurse.
log_write_failed() {
    local lost="$LOG_FILE"
    LOG_ENABLED=""
    warn "could not write to the log file $lost, continuing without a log"
    return 0
}

# Under --verbose the log-only detail is echoed to stderr as well.
verbose_console() {
    [ -n "$VERBOSE" ] || return 0
    printf '    %s%s %s%s\n' "$DIM" "$1" "$2" "$RESET" >&2
}

# log_info / log_debug / log_warn are for detail that is NOT already on the
# console. The console helpers below call log_write directly instead, so a
# verbose run does not print every message twice.
log_info() {
    log_write INFO "$*"
    verbose_console "INFO" "$*"
}

log_debug() {
    log_write DEBUG "$*"
    verbose_console "DEBUG" "$*"
}

log_warn() {
    log_write WARN "$*"
    verbose_console "WARN" "$*"
}

# ---------------------------------------------------------------------------
# Console output
# ---------------------------------------------------------------------------
#
# Every diagnostic goes to stderr. Anything this script prints on stdout would
# end up inside a caller's pipeline, and installers get piped a lot. Each of
# these mirrors its message into the log without the colour codes.
#
# The layout is two blocks of aligned label/value rows, and these printers are
# the only place that knows how wide those labels are. Business logic below
# calls fact/step_ok/say and never counts a space, because hand-counted padding
# drifts the first time somebody renames a label.
#
#     Proa CLI installer
#
#       platform   Linux aarch64 (aarch64-unknown-linux-gnu)     <- FACT_WIDTH
#
#       ok checksum      sha256 verified                         <- STEP_WIDTH
#
# The two widths differ because the step rows lose two columns to the glyph.
# FACT_WIDTH, STEP_WIDTH and LINK_WIDTH are set in set_defaults with everything
# else: nothing in this file runs outside a function, so that a truncated
# download leaves a script that defines helpers and does nothing.

blank() {
    printf '\n' >&2
}

# A headline: flush left, bold, and the only thing on its line. There are three
# in the whole script ("Proa CLI installer", "proa X is ready.", "Installation
# failed."), which is what makes them findable in a scrollback.
title() {
    printf '%s%s%s\n' "$BOLD" "$*" "$RESET" >&2
    log_write INFO "$*"
}

# A label/value row. $1 is the column width, which is a constant per block so
# that every value in that block starts in the same column, and printf does the
# padding.
#
# The colour codes sit outside the padded field on purpose. A field width
# counts the bytes of its argument, so a label wrapped in escapes would be
# measured with them and lose that many spaces of padding. Written this way the
# call emits the same columns whether colour is on or off, which is what keeps a
# redirected run readable.
row() {
    local width="$1"
    local label="$2"
    local value="$3"

    printf "  %s%-${width}s%s  %s\n" "$DIM" "$label" "$RESET" "$value" >&2
}

# One of the platform facts printed under the headline.
fact() {
    row "$FACT_WIDTH" "$1" "$2"
    log_write INFO "$1: $2"
}

# A step row: glyph, then the same aligned label/value shape. $1 glyph, $2 the
# colour for it, $3 label, $4 value. The glyph is coloured and the label dimmed,
# so the value is the brightest thing on the line and a column of values reads
# top to bottom on its own.
step_line() {
    printf "  %s%s%s %s%-${STEP_WIDTH}s%s  %s\n" \
        "$2" "$1" "$RESET" "$DIM" "$3" "$RESET" "$4" >&2
    log_write INFO "$3: $4"
}

# Something is happening or has happened. No colour: an arrow is not a verdict.
step_run() {
    # Dim rather than no colour: an in-progress row should read as quieter than
    # the green completed rows, and passing an empty colour here would still
    # emit a reset with nothing opened before it.
    step_line "$GLYPH_STEP" "$DIM" "$1" "$2"
}

# A step that succeeded.
step_ok() {
    step_line "$GLYPH_OK" "$GREEN" "$1" "$2"
}

# A step that did not, on a path where the install still continues.
step_fail() {
    step_line "$GLYPH_FAIL" "$RED" "$1" "$2"
}

# Prose under a headline, indented to the same column as the label blocks.
say() {
    printf '  %s\n' "$*" >&2
    log_write INFO "$*"
}

# Secondary prose: the same place, dimmed.
detail() {
    printf '  %s%s%s\n' "$DIM" "$*" "$RESET" >&2
    log_write INFO "$*"
}

# A command the reader is meant to run, indented one step further so it is
# obviously a thing to copy rather than a sentence.
cmd_line() {
    printf '    %s\n' "$*" >&2
    log_write INFO "$*"
}

# The dim label/value pairs that close a run (Docs, Log).
link() {
    printf "  %s%-${LINK_WIDTH}s  %s%s\n" "$DIM" "$1" "$2" "$RESET" >&2
}

# Warnings keep the label column of the facts block rather than a colour of
# their own: the palette has no fourth colour to spend, and a warning is not a
# failure. The message stays at full brightness so it still reads.
warn() {
    printf "  %-${FACT_WIDTH}s  %s\n" "warning" "$*" >&2
    log_write WARN "$*"
}

# Runs the printer named in $2 over every line of $1, which may hold several
# separated by $NL. Messages that want two hashes under each other, or a
# sentence and then a command, are written as one string and laid out here
# rather than as a run of calls at the call site.
print_lines() {
    local rest="$1"
    local line

    while :; do
        case "$rest" in
            *"$NL"*)
                line="${rest%%"$NL"*}"
                rest="${rest#*"$NL"}"
                ;;
            *)
                line="$rest"
                rest=""
                ;;
        esac

        "$2" "$line"
        [ -n "$rest" ] || break
    done
}

# One line of a failure body: indented like the blocks above, blank when blank
# rather than an indented run of trailing spaces, and mirrored into the log only
# when it carries text, so a message formatted for a human leaves no empty
# records in a file meant to be grepped.
fail_line() {
    if [ -n "$1" ]; then
        printf '  %s\n' "$1" >&2
        log_write ERROR "$1"
    else
        printf '\n' >&2
    fi
}

# The failure headline, printed once however the run dies. err() calls it, and
# so does the exit trap for the paths that exit without going through err (a
# Ctrl-C during a download, most of all), so a non-zero exit never ends with an
# unexplained log path and nothing else.
fail_headline() {
    [ -z "$FAILURE_SHOWN" ] || return 0
    FAILURE_SHOWN="yes"

    blank
    title "Installation failed."
    blank
}

# $1 may hold several lines separated by $NL. Multi-line failures exist so a
# reader can compare two hashes down the page instead of inside one wrapped
# sentence; see verify_checksum.
err() {
    local message="$*"

    fail_headline
    print_lines "$message" fail_line
    log_footer "failure" 1
    print_log_hint_failure
    exit 1
}

# ---------------------------------------------------------------------------
# Display formatting
# ---------------------------------------------------------------------------

# Prints $1 bytes the way a person reads them: "512 B", "10.4 MB". The log
# keeps the raw count, where precision beats looks.
#
# POSIX sh has no floating point, so the decimal is placed by hand. The
# remainder is divided by a tenth of the unit rather than multiplied by ten,
# because a gigabyte-sized multiply overflows the signed long that shell
# arithmetic is only guaranteed to give us.
format_bytes() {
    local n="$1"
    local div
    local unit
    local whole
    local rem
    local frac

    case "$n" in
        "" | *[!0-9]*)
            printf 'unknown size'
            return 0
            ;;
    esac

    if [ "$n" -lt 1024 ]; then
        printf '%s B' "$n"
        return 0
    fi

    div=1024
    unit="KB"

    while :; do
        whole=$((n / div))
        rem=$((n % div))
        # + div/20 is half a tenth, so this rounds rather than truncates.
        frac=$(((rem + div / 20) / (div / 10)))
        if [ "$frac" -ge 10 ]; then
            whole=$((whole + 1))
            frac=0
        fi

        # The unit is picked after rounding, not before. 1048575 bytes is
        # 1023.99 KB, which rounds to "1024.0 KB": a number the next unit up
        # exists precisely to avoid printing.
        if [ "$whole" -lt 1024 ] || [ "$unit" = "GB" ]; then
            break
        fi

        case "$unit" in
            KB)
                div=1048576
                unit="MB"
                ;;
            MB)
                div=1073741824
                unit="GB"
                ;;
        esac
    done

    printf '%s.%s %s' "$whole" "$frac" "$unit"
}

# Shortens $HOME to ~ for display only. Every log record keeps the full path,
# because a bug report that says "~/.proa/bin" does not say whose home.
tilde_path() {
    case "$1" in
        "$HOME") printf '~' ;;
        "$HOME"/*) printf '~%s' "${1#"$HOME"}" ;;
        *) printf '%s' "$1" ;;
    esac
}

# Drops the scheme from a URL for the facts block. The full URL is logged.
display_url() {
    local url="$1"
    printf '%s' "${url#https://}"
}

check_cmd() {
    command -v "$1" > /dev/null 2>&1
}

# $1 command, $2 short explanation of why it is needed.
need_cmd() {
    if ! check_cmd "$1"; then
        err "'$1' is required but was not found on PATH ($2)"
    fi
}

# ---------------------------------------------------------------------------
# Elapsed time
# ---------------------------------------------------------------------------
#
# shellcheck disable=SC3028
# SC3028 fires on every mention of SECONDS below. That is the point: SECONDS
# is outside POSIX, every read of it is guarded by ${SECONDS:-} and by
# HAVE_SECONDS, and the date arithmetic below is the fallback for the shells
# that do not have it.

init_timer() {
    # SECONDS is not POSIX. dash and busybox ash leave it unset, and the
    # shells that do implement it have it set before the script assigns
    # anything, which is what this tests for.
    if [ -n "${SECONDS:-}" ]; then
        SECONDS=0
        HAVE_SECONDS="yes"
    fi

    START_EPOCH=$(date +%s 2> /dev/null) || START_EPOCH=""
    case "$START_EPOCH" in
        "" | *[!0-9]*) START_EPOCH="" ;;
    esac
}

# Prints whole seconds since init_timer, or returns non-zero when this machine
# offers no way to tell the time.
# shellcheck disable=SC3028
elapsed_seconds() {
    local now

    if [ -n "$HAVE_SECONDS" ]; then
        case "${SECONDS:-}" in
            "" | *[!0-9]*) ;;
            *)
                printf '%s' "$SECONDS"
                return 0
                ;;
        esac
    fi

    if [ -n "$START_EPOCH" ]; then
        now=$(date +%s 2> /dev/null) || now=""
        case "$now" in
            "" | *[!0-9]*) ;;
            *)
                printf '%s' "$((now - START_EPOCH))"
                return 0
                ;;
        esac
    fi

    return 1
}

# ---------------------------------------------------------------------------
# Log file lifecycle
# ---------------------------------------------------------------------------

# $1 directory. Prints the default log path inside it.
default_log_path() {
    local stamp=""

    if [ -n "$LOG_HAVE_DATE" ]; then
        stamp=$(date -u +%Y%m%dT%H%M%SZ 2> /dev/null) || stamp=""
    fi

    if [ -n "$stamp" ]; then
        printf '%s/proa-install-%s-%s.log' "$1" "$stamp" "$$"
    else
        printf '%s/proa-install-%s.log' "$1" "$$"
    fi
}

# Creates (or opens for append) the log file. Returns non-zero if it cannot.
open_log() {
    local path="$1"
    local dir

    case "$path" in
        */*)
            dir="${path%/*}"
            [ -n "$dir" ] || dir="/"
            mkdir -p "$dir" 2> /dev/null || true
            ;;
    esac

    # Append rather than truncate. Someone who points PROA_LOG at an existing
    # file should get a second run appended to it, never a file they lose.
    # The redirect runs in a subshell so a failure is a return value here
    # instead of `set -e` killing the install over a log file.
    ( : >> "$path" ) 2> /dev/null || return 1
    return 0
}

# --verbose has to be known before the header is written, and parse_args runs
# after the log is open. This looks for the one flag and nothing else; every
# other flag, and all validation, still belongs to parse_args.
prescan_verbose() {
    local arg

    for arg in "$@"; do
        case "$arg" in
            --verbose) VERBOSE="yes" ;;
        esac
    done
}

init_log() {
    local requested="${PROA_LOG:-}"

    if check_cmd date && date -u +%Y-%m-%dT%H:%M:%SZ > /dev/null 2>&1; then
        LOG_HAVE_DATE="yes"
    fi

    DEFAULT_LOG_FILE=$(default_log_path /tmp)

    case "$requested" in
        [Oo][Ff][Ff])
            # Exact string "off", any case. Nothing is written and nothing is
            # printed about the log for the rest of the run.
            LOG_SOURCE="environment (off)"
            LOG_FILE=""
            LOG_ENABLED=""
            return 0
            ;;
        "")
            LOG_FILE="$DEFAULT_LOG_FILE"
            LOG_SOURCE="default"
            ;;
        *)
            LOG_FILE="$requested"
            LOG_SOURCE="environment"
            ;;
    esac

    if open_log "$LOG_FILE"; then
        LOG_ENABLED="yes"
    elif [ "$LOG_SOURCE" = "default" ] && [ -n "${TMPDIR:-}" ] && [ "${TMPDIR%/}" != "/tmp" ]; then
        # A read-only or full /tmp is worth one retry in TMPDIR before giving
        # up, but only when the caller did not name a path themselves.
        LOG_FILE=$(default_log_path "${TMPDIR%/}")
        DEFAULT_LOG_FILE="$LOG_FILE"
        if open_log "$LOG_FILE"; then
            LOG_ENABLED="yes"
        fi
    fi

    if [ -z "$LOG_ENABLED" ]; then
        warn "could not create a log file at $LOG_FILE, continuing with console output only"
        LOG_FILE=""
        return 0
    fi

    log_header "$@"
}

log_header() {
    local stamp

    log_info "=== proa installer ==="
    log_info "script: install.sh (revision $INSTALLER_REVISION)"
    if [ $# -gt 0 ]; then
        log_info "invocation: $0 $*"
    else
        log_info "invocation: $0 (no arguments)"
    fi
    stamp=$(utc_now)
    if [ -n "$stamp" ]; then
        log_info "started: $stamp"
    else
        log_info "started: unknown, this machine has no usable 'date'"
    fi
    log_info "log file: $LOG_FILE (from $LOG_SOURCE)"
    log_console_mode
    log_info "This file is meant to be attached to a bug report:"
    log_info "  https://github.com/$REPO/issues"
    log_info "The environment is never dumped. The PROA_* and NO_COLOR settings"
    log_info "below are recorded by name; CARGO_HOME and the cargo registry index"
    log_info "appear only on a source build, and the index is recorded with any"
    log_info "credentials redacted."
}

# What the console decided about itself. Worth a record, because "the output
# was full of question marks" and "everything was one colour" are both reports
# this line answers on its own.
log_console_mode() {
    local tty="not a terminal"
    local colour="off"
    local glyphs="ascii"

    [ -z "$IS_TTY" ] || tty="a terminal"
    [ -z "$RESET" ] || colour="on"
    [ -z "$UTF8_GLYPHS" ] || glyphs="utf-8"

    log_info "console: stdout is $tty, colour $colour, glyphs $glyphs"
}

# $1 name, $2 value, $3 where it came from.
log_input() {
    local value="$2"
    [ -n "$value" ] || value="<unset>"
    log_info "  $1 = $value (from $3)"
}

log_inputs() {
    log_info "inputs in effect (flags win over the environment):"
    log_input "PROA_VERSION" "$VERSION" "$SRC_VERSION"
    log_input "PROA_HOME" "$PROA_ROOT" "$SRC_HOME"
    log_input "PROA_INSTALL_DIR" "$INSTALL_DIR" "$SRC_INSTALL_DIR"
    # Logged next to each other, with the split spelled out, because a report
    # that says "I pointed --repo at my fork and it still downloaded the
    # official build" is otherwise impossible to diagnose from this file.
    log_input "PROA_DOWNLOAD_BASE" "$DOWNLOAD_BASE" "$SRC_DOWNLOAD_BASE"
    log_info "    every Proa CLI download and the latest-version lookup goes here"
    log_input "PROA_REPO" "$REPO" "$SRC_REPO"
    log_info "    source fallback only (cargo install --git), never a download"
    log_input "PROA_NO_MODIFY_PATH" "$NO_MODIFY_PATH" "$SRC_NO_MODIFY_PATH"
    log_input "PROA_BUILD_FROM_SOURCE" "$BUILD_FROM_SOURCE" "$SRC_BUILD_FROM_SOURCE"
    log_input "PROA_PREBUILT_ONLY" "$PREBUILT_ONLY" "$SRC_PREBUILT_ONLY"
    log_input "PROA_TAILWIND" "$TAILWIND_CHOICE" "$SRC_TAILWIND"
    log_input "tailwind-only mode" "$TAILWIND_ONLY" "$SRC_TAILWIND_ONLY"
    log_input "PROA_YES" "$ASSUME_YES" "$SRC_YES"
    log_input "PROA_VERBOSE" "$VERBOSE" "$SRC_VERBOSE"
    log_input "NO_COLOR" "${NO_COLOR:-}" "$SRC_NO_COLOR"
    case "${PROA_LOG:-}" in
        [Oo][Ff][Ff]) log_input "PROA_LOG" "off, no log file is written" "environment" ;;
        *)
            if [ -n "$LOG_ENABLED" ]; then
                log_input "PROA_LOG" "$LOG_FILE" "$LOG_SOURCE"
            else
                log_input "PROA_LOG" "unavailable, this run has no log file" "$LOG_SOURCE"
            fi
            ;;
    esac
}

# A blank line and a "--- <name> ---" marker between the phases of the log:
# header, platform, download, install, summary. This is the only formatting the
# log has. Records themselves stay one per line, unaligned and unchanged, so
# that grep and a pair of human eyes both keep working on a pasted file.
#
# Asking for the section that is already open does nothing, so a phase that can
# be entered from two places (a download that either goes ahead or falls back to
# a build) still opens exactly one section.
#
# The blank line is written straight rather than through log_write, since a
# timestamped empty record is not a separator.
log_section() {
    [ -n "$LOG_ENABLED" ] || return 0
    [ "$LOG_SECTION" != "$1" ] || return 0
    LOG_SECTION="$1"

    printf '\n' >> "$LOG_FILE" 2> /dev/null || log_write_failed
    log_info "--- $1 ---"
}

# $1 outcome word, $2 exit code. Written once, whichever path gets here first.
log_footer() {
    local secs

    [ -z "$LOG_FOOTER_DONE" ] || return 0
    LOG_FOOTER_DONE="yes"

    log_section "summary"
    log_info "outcome: $1"
    log_info "exit code: $2"
    if secs=$(elapsed_seconds); then
        log_info "elapsed: ${secs}s"
    else
        log_info "elapsed: unavailable, this machine has no usable clock"
    fi
    log_info "=== end of proa installer log ==="
}

# The last thing an error prints. A user who never sees this line will not
# think to attach the file.
print_log_hint_failure() {
    [ -n "$LOG_ENABLED" ] || return 0
    [ -z "$LOG_HINT_DONE" ] || return 0
    LOG_HINT_DONE="yes"

    blank
    link "Log" "$LOG_FILE"
    # Printed rather than said: log_footer has already written the end marker,
    # and records after it would look like a second run had started.
    printf '  Please attach the log if you report this:\n' >&2
    printf '    https://github.com/%s/issues\n' "$REPO" >&2
}

# One quiet line on success, so the path is discoverable without being loud.
# It lands directly under the Docs line print_summary just printed, which is
# why it is a link row and not a sentence.
print_log_hint_success() {
    [ -n "$LOG_ENABLED" ] || return 0
    [ -z "$LOG_HINT_DONE" ] || return 0
    LOG_HINT_DONE="yes"

    link "Log" "$LOG_FILE"
}

# --help should not leave a file behind, so the log it opened is removed again.
# A path the caller chose with PROA_LOG is left alone: they asked for it.
# Printing an outcome is suppressed too, because --help has no outcome.
discard_log() {
    if [ -n "$LOG_ENABLED" ] && [ "$LOG_SOURCE" = "default" ] && [ -n "$LOG_FILE" ]; then
        rm -f "$LOG_FILE" 2> /dev/null || true
    fi
    LOG_ENABLED=""
    LOG_FILE=""
    LOG_FOOTER_DONE="yes"
    LOG_HINT_DONE="yes"
}

# ---------------------------------------------------------------------------
# Usage
# ---------------------------------------------------------------------------

usage() {
    printf '%s\n' \
"proa installer" \
"" \
"Installs the Proa CLI (the 'proa' and 'cargo-proa' binaries) from a prebuilt" \
"archive, or builds it from source when no prebuilt binary matches this" \
"machine." \
"" \
"Prebuilt archives are published to GitHub Releases and served through" \
"https://proa.so/downloads/cli, which is what this script downloads from." \
"" \
"USAGE" \
"    curl -fsSL https://proa.so/install.sh | sh" \
"    curl -fsSL https://proa.so/install.sh | sh -s -- [OPTIONS]" \
"    ./install.sh [OPTIONS]" \
"" \
"OPTIONS" \
"    --version <VERSION>   Install this version instead of the latest release." \
"                          Accepts 0.1.0, v0.1.0 or proa-cli-v0.1.0." \
"    --to <DIR>            Install the binaries into DIR instead of" \
"                          \$PROA_HOME/bin (default \$HOME/.proa/bin)." \
"    --download-base <URL> Download prebuilt archives from URL instead of" \
"                          https://proa.so/downloads/cli. This is the setting" \
"                          to change to install from a mirror, a fork's" \
"                          bridge, or a local test server. Must be https, and" \
"                          must serve latest.json and <version>/<file>." \
"                          A trailing slash is ignored." \
"    --repo <OWNER/NAME>   Build from a different GitHub repository when the" \
"                          install falls back to source. This does NOT affect" \
"                          downloads; use --download-base for those." \
"                          Default: Proa-Labs/proa" \
"    --no-modify-path      Never write to a shell profile. The installer still" \
"                          prints the line to add." \
"    --build-from-source   Skip the prebuilt download and run" \
"                          'cargo install'. With no version, build Git HEAD;" \
"                          a version pins the release or snapshot source." \
"                          Requires cargo and takes several minutes." \
"    --prebuilt-only       Fail if the matching release archive cannot be" \
"                          downloaded. Never fall back to a source build." \
"    --with-tailwind       Also install the pinned Tailwind CSS CLI" \
"                          v$TAILWIND_VERSION standalone binary." \
"    --no-tailwind         Do not ask about or install the Tailwind CSS CLI." \
"    --tailwind-only       Install only Tailwind CSS CLI v$TAILWIND_VERSION." \
"                          Skips Proa downloads and Rust toolchain checks." \
"    --verbose             Also print to the terminal the detail that normally" \
"                          only goes to the log file." \
"    -y, --yes             Assume yes for every prompt. Use this in CI." \
"    -h, --help            Print this help and exit." \
"" \
"ENVIRONMENT" \
"    PROA_VERSION            Same as --version." \
"    PROA_HOME               Install root. Default \$HOME/.proa; binaries go" \
"                            into \$PROA_HOME/bin." \
"    PROA_INSTALL_DIR        Explicit bin directory. Overrides PROA_HOME." \
"    PROA_DOWNLOAD_BASE      Same as --download-base." \
"    PROA_REPO               Same as --repo." \
"    PROA_NO_MODIFY_PATH     Any non-empty value behaves like --no-modify-path." \
"    PROA_BUILD_FROM_SOURCE  Any non-empty value behaves like --build-from-source." \
"    PROA_PREBUILT_ONLY      Any non-empty value behaves like --prebuilt-only." \
"    PROA_TAILWIND           Tailwind choice: ask (default), install, or skip." \
"    PROA_YES                Any non-empty value behaves like --yes." \
"    PROA_VERBOSE            Any non-empty value behaves like --verbose." \
"    PROA_LOG                Write the log here instead of the default path." \
"                            Set it to 'off' to write no log at all." \
"    NO_COLOR                Any value disables coloured output." \
"" \
"    Flags win over environment variables." \
"" \
"WHERE THINGS COME FROM" \
"    PROA_DOWNLOAD_BASE and PROA_REPO are not the same knob, and setting one" \
"    does not move the other." \
"" \
"    PROA_DOWNLOAD_BASE is the only setting Proa CLI downloads use. This" \
"    script asks it" \
"    for <base>/latest.json to learn the newest version, then fetches" \
"    <base>/<version>/proa-cli-<version>-<target>.tar.gz and" \
"    <base>/<version>/proa-cli-<version>-SHA256SUMS. The default," \
"    https://proa.so/downloads/cli, is a bridge in front of GitHub Releases," \
"    which is where those archives actually live and stay." \
"" \
"    PROA_REPO is only used when the install falls back to a source build," \
"    where it becomes 'cargo install --git https://github.com/<repo>'. Pointing" \
"    it at a fork changes what gets compiled, not what gets downloaded." \
"" \
"LOGGING" \
"    Every run writes a log to:" \
"" \
"        /tmp/proa-install-<UTC timestamp>-<pid>.log" \
"" \
"    It holds every URL fetched and its result, the expected and computed" \
"    checksums, the detected platform and toolchain, which shell profiles were" \
"    touched, and the reason behind every fallback. The path is printed when" \
"    the run finishes; attach that file to a bug report." \
"" \
"    The log never dumps your environment. It records the PROA_* and NO_COLOR" \
"    settings listed above by name, and on a source build the cargo registry" \
"    index with any credentials in it redacted, so it cannot carry a token or" \
"    a password." \
"    If the log cannot be created the install continues without it." \
"" \
"WHAT IT DOES" \
"    1. Detects your OS and CPU and picks the matching release archive." \
"    2. Checks for cargo and rustc. Proa builds your project with cargo, so a" \
"       missing toolchain stops the install; a toolchain older than Rust" \
"       $MSRV is reported as a warning." \
"    3. Reads <download base>/latest.json for the newest version, unless" \
"       --version already named one." \
"    4. Downloads the archive and its SHA256SUMS file over HTTPS and verifies" \
"       the checksum before anything is unpacked. A mismatch installs nothing." \
"    5. Installs 'proa' and 'cargo-proa', then optionally downloads the pinned" \
"       Tailwind CSS CLI v$TAILWIND_VERSION binary from its official release." \
"    6. Adds the bin directory to your shell profile if it is not already on" \
"       PATH." \
"" \
"    --tailwind-only skips the Proa and Rust steps and performs only the" \
"    verified Tailwind download, installation, and PATH setup." \
"" \
"    Machines with no prebuilt archive (musl, FreeBSD, 32-bit) fall back to" \
"    building from source automatically, and so does a run that cannot reach" \
"    the download base or finds no published release there. --prebuilt-only" \
"    turns each fallback into an error, which is useful in CI." \
"" \
"EXAMPLES" \
"    curl -fsSL https://proa.so/install.sh | sh -s -- --yes" \
"    curl -fsSL https://proa.so/install.sh | sh -s -- --with-tailwind" \
"    curl -fsSL https://proa.so/install.sh | sh -s -- --tailwind-only" \
"    curl -fsSL https://proa.so/install.sh | PROA_VERSION=0.1.0 sh" \
"    curl -fsSL https://proa.so/install.sh | PROA_PREBUILT_ONLY=1 sh" \
"    ./install.sh --to /usr/local/bin --no-modify-path" \
"    ./install.sh --verbose" \
"    ./install.sh --download-base https://staging.proa.so/downloads/cli" \
"    ./install.sh --build-from-source --repo my-org/proa" \
"    PROA_LOG=./install.log ./install.sh"
}

parse_args() {
    while [ $# -gt 0 ]; do
        case "$1" in
            -h | --help)
                usage
                # A help run should not litter /tmp with a log of itself.
                discard_log
                exit 0
                ;;
            --version)
                shift
                [ $# -gt 0 ] || err "--version needs a value, for example --version 0.1.0"
                VERSION="$1"
                SRC_VERSION="flag"
                ;;
            --version=*)
                VERSION="${1#*=}"
                SRC_VERSION="flag"
                ;;
            --to)
                shift
                [ $# -gt 0 ] || err "--to needs a directory, for example --to /usr/local/bin"
                INSTALL_DIR="$1"
                DIR_WAS_EXPLICIT="yes"
                SRC_INSTALL_DIR="flag"
                ;;
            --to=*)
                INSTALL_DIR="${1#*=}"
                DIR_WAS_EXPLICIT="yes"
                SRC_INSTALL_DIR="flag"
                ;;
            --download-base)
                shift
                [ $# -gt 0 ] || err "--download-base needs a URL, for example --download-base https://proa.so/downloads/cli"
                DOWNLOAD_BASE="$1"
                SRC_DOWNLOAD_BASE="flag"
                ;;
            --download-base=*)
                DOWNLOAD_BASE="${1#*=}"
                SRC_DOWNLOAD_BASE="flag"
                ;;
            --repo)
                shift
                [ $# -gt 0 ] || err "--repo needs an owner/name slug, for example --repo Proa-Labs/proa"
                REPO="$1"
                SRC_REPO="flag"
                ;;
            --repo=*)
                REPO="${1#*=}"
                SRC_REPO="flag"
                ;;
            --no-modify-path)
                NO_MODIFY_PATH="yes"
                SRC_NO_MODIFY_PATH="flag"
                ;;
            --build-from-source)
                BUILD_FROM_SOURCE="yes"
                SRC_BUILD_FROM_SOURCE="flag"
                ;;
            --prebuilt-only)
                PREBUILT_ONLY="yes"
                SRC_PREBUILT_ONLY="flag"
                ;;
            --with-tailwind)
                [ "$TAILWIND_FLAG" != "skip" ] || err "--with-tailwind and --no-tailwind cannot be combined"
                TAILWIND_CHOICE="install"
                TAILWIND_FLAG="install"
                SRC_TAILWIND="flag"
                ;;
            --tailwind-only)
                [ "$TAILWIND_FLAG" != "skip" ] || err "--tailwind-only and --no-tailwind cannot be combined"
                TAILWIND_ONLY="yes"
                TAILWIND_CHOICE="install"
                TAILWIND_FLAG="install"
                SRC_TAILWIND="flag"
                SRC_TAILWIND_ONLY="flag"
                ;;
            --no-tailwind)
                if [ -n "$TAILWIND_ONLY" ]; then
                    err "--tailwind-only and --no-tailwind cannot be combined"
                fi
                [ "$TAILWIND_FLAG" != "install" ] || err "--with-tailwind and --no-tailwind cannot be combined"
                TAILWIND_CHOICE="skip"
                TAILWIND_FLAG="skip"
                SRC_TAILWIND="flag"
                ;;
            --verbose)
                VERBOSE="yes"
                SRC_VERBOSE="flag"
                ;;
            -y | --yes)
                ASSUME_YES="yes"
                SRC_YES="flag"
                ;;
            *)
                err "unknown option '$1'. Run with --help to see the supported flags."
                ;;
        esac
        shift
    done

    if [ -n "${PROA_INSTALL_DIR:-}" ] && [ -z "$DIR_WAS_EXPLICIT" ]; then
        DIR_WAS_EXPLICIT="yes"
    fi

    # Flags replace the environment before this check, so a valid flag can
    # recover from a stale or misspelled PROA_TAILWIND value.
    case "$TAILWIND_CHOICE" in
        [Aa][Ss][Kk]) TAILWIND_CHOICE="ask" ;;
        [Ii][Nn][Ss][Tt][Aa][Ll][Ll] | [Yy][Ee][Ss]) TAILWIND_CHOICE="install" ;;
        [Ss][Kk][Ii][Pp] | [Nn][Oo]) TAILWIND_CHOICE="skip" ;;
        *) err "PROA_TAILWIND expects ask, install, or skip; got '$TAILWIND_CHOICE'" ;;
    esac

    if [ -n "$TAILWIND_ONLY" ] && [ -n "$BUILD_FROM_SOURCE" ]; then
        err "--tailwind-only and --build-from-source cannot be combined"
    fi
    if [ -n "$TAILWIND_ONLY" ] && [ -n "$PREBUILT_ONLY" ]; then
        err "--tailwind-only and --prebuilt-only cannot be combined"
    fi
    if [ -n "$BUILD_FROM_SOURCE" ] && [ -n "$PREBUILT_ONLY" ]; then
        err "--build-from-source and --prebuilt-only cannot be combined"
    fi

    # The scheme is checked rather than assumed, because every request this
    # script makes is HTTPS-only at the transport level too: curl gets
    # --proto '=https' and wget gets --https-only. An http:// base would be
    # refused by the downloader with an error that says nothing about which
    # setting caused it, so it is rejected here where the message can name it.
    case "$DOWNLOAD_BASE" in
        https://?*) ;;
        http://*)
            err "--download-base must be an https:// URL. This installer refuses plain HTTP for downloads, so '$DOWNLOAD_BASE' could never be fetched."
            ;;
        *)
            err "--download-base expects an https:// URL such as https://proa.so/downloads/cli, got '$DOWNLOAD_BASE'"
            ;;
    esac

    # A base written with a trailing slash is the obvious way to write it, and
    # every URL below appends "/something", so it is normalised once here
    # instead of producing "…/cli//latest.json". The loop covers the copy and
    # paste that ends in more than one.
    while :; do
        case "$DOWNLOAD_BASE" in
            https://) break ;;
            */) DOWNLOAD_BASE="${DOWNLOAD_BASE%/}" ;;
            *) break ;;
        esac
    done
    case "$DOWNLOAD_BASE" in
        https://?*) ;;
        *) err "--download-base needs a host after https://, got only '$DOWNLOAD_BASE'" ;;
    esac

    # REPO is interpolated into the git URL the source fallback clones, and
    # VERSION into download URLs and into file names under the temporary
    # directory, so both are held to what GitHub actually allows. Without this a
    # value containing '/' or '..' produces a nonsense URL and a destination
    # path pointing outside the directory that gets cleaned up, and the
    # resulting error message explains none of it.
    case "$REPO" in
        *[!A-Za-z0-9._-]*/*[!A-Za-z0-9._-]* | */*/* | *..* | /* | */ | *[!A-Za-z0-9._/-]*)
            err "--repo expects an owner/name slug such as Proa-Labs/proa, got '$REPO'"
            ;;
        */*) ;;
        *) err "--repo expects an owner/name slug such as Proa-Labs/proa, got '$REPO'" ;;
    esac

    # Accept the tag, the tag without the prefix, or a bare version.
    if [ -n "$VERSION" ]; then
        VERSION="${VERSION#proa-cli-}"
        VERSION="${VERSION#v}"
        if ! is_release_version "$VERSION"; then
            err "--version expects a release version such as 0.1.0, v0.1.0 or proa-cli-v0.1.0, got '$VERSION'"
        fi
    fi
}

# A release version starts with a digit and is limited to the characters a
# semver tag can hold. Used for both the flag and the version latest.json
# reports, because both end up in a URL and in a path under TMP_DIR.
is_release_version() {
    case "$1" in
        "" | *[!0-9A-Za-z.+-]*) return 1 ;;
        [0-9]*) return 0 ;;
        *) return 1 ;;
    esac
}

# ---------------------------------------------------------------------------
# Interactivity
# ---------------------------------------------------------------------------

# When the script is piped into sh its stdin is the script itself, so stdin is
# useless for both testing interactivity and reading answers. The controlling
# terminal is the only reliable channel.
is_interactive() {
    if [ ! -t 2 ]; then
        log_debug "not interactive: stderr is not a terminal"
        return 1
    fi
    if ! ( exec 3< /dev/tty ) 2> /dev/null; then
        log_debug "not interactive: /dev/tty could not be opened"
        return 1
    fi
    log_debug "interactive: /dev/tty is available for prompts"
    return 0
}

# $1 question. Defaults to yes on an empty answer.
prompt_yes_no() {
    local reply
    # Indented to the same column as everything else between the headlines.
    printf '  %s [Y/n] ' "$1" > /dev/tty
    reply=""
    read -r reply < /dev/tty || reply=""
    case "$reply" in
        "" | y | Y | yes | Yes | YES)
            log_info "prompt: $1 -> yes (answer '$reply')"
            return 0
            ;;
        *)
            log_info "prompt: $1 -> no (answer '$reply')"
            return 1
            ;;
    esac
}

# ---------------------------------------------------------------------------
# Platform detection
# ---------------------------------------------------------------------------

# musl systems cannot run the gnu archives, so they take the source path.
# LIBC_EVIDENCE records which of the three probes answered, because "we decided
# you were musl" is useless in a bug report without the reason.
is_musl() {
    local ldd_out

    if [ -f /etc/alpine-release ]; then
        LIBC_EVIDENCE="/etc/alpine-release exists"
        return 0
    fi

    # ldd answers directly on both glibc and musl. On musl it writes its banner
    # to stderr and exits non-zero, so the failure has to be absorbed *inside*
    # the substitution: a trailing `|| ldd_out=""` would run on exactly the
    # musl systems this is looking for and throw away the banner it just
    # captured, leaving the branch dead. `|| true` also covers a missing ldd.
    ldd_out=$( (ldd --version) 2>&1 || true )
    log_debug "ldd --version: $(first_line "$ldd_out")"
    case "$ldd_out" in
        *musl*)
            LIBC_EVIDENCE="'ldd --version' mentions musl"
            return 0
            ;;
    esac

    # Minimal images often ship no ldd, so check the loader itself. The name is
    # spelled out rather than globbed because zsh aborts on an unmatched glob.
    if [ -e "/lib/ld-musl-$(uname -m).so.1" ]; then
        LIBC_EVIDENCE="/lib/ld-musl-$(uname -m).so.1 exists"
        return 0
    fi

    LIBC_EVIDENCE="no /etc/alpine-release, ldd did not mention musl, no /lib/ld-musl-*.so.1"
    return 1
}

# Sets OS_NAME, ARCH_NAME and TARGET. An empty TARGET means "no prebuilt asset
# exists for this machine", which is a fallback condition and not an error.
detect_platform() {
    need_cmd uname "used to detect your operating system and CPU"

    OS_NAME=$(uname -s)
    ARCH_NAME=$(uname -m)
    UNAME_S="$OS_NAME"
    UNAME_M="$ARCH_NAME"

    log_info "uname -s: $UNAME_S"
    log_info "uname -m: $UNAME_M"
    log_debug "uname -a: $(uname -a 2> /dev/null || printf 'unavailable')"

    case "$OS_NAME" in
        Linux)
            if is_musl; then
                LIBC_KIND="musl"
                log_info "libc: $LIBC_KIND ($LIBC_EVIDENCE)"
                TARGET=""
                TARGET_REASON="musl systems cannot run the gnu prebuilt archives"
                log_info "target triple: none, $TARGET_REASON"
                return 0
            fi
            LIBC_KIND="glibc"
            log_info "libc: $LIBC_KIND ($LIBC_EVIDENCE)"
            case "$ARCH_NAME" in
                x86_64 | amd64) TARGET="x86_64-unknown-linux-gnu" ;;
                aarch64 | arm64) TARGET="aarch64-unknown-linux-gnu" ;;
                *)
                    TARGET=""
                    TARGET_REASON="no prebuilt archive is published for Linux $ARCH_NAME"
                    ;;
            esac
            ;;
        Darwin)
            LIBC_KIND="Apple libSystem"
            LIBC_EVIDENCE="uname -s reported Darwin, the glibc/musl split does not apply"
            log_info "libc: $LIBC_KIND ($LIBC_EVIDENCE)"
            case "$ARCH_NAME" in
                x86_64)
                    # Rosetta reports x86_64 from an arm64 kernel, and the
                    # native archive is the better install in that case.
                    if [ "$(sysctl -n sysctl.proc_translated 2> /dev/null || printf '0')" = "1" ]; then
                        TARGET="aarch64-apple-darwin"
                        # The platform row already ends in a parenthesised
                        # triple, so this one says its piece without a second
                        # pair of brackets. The log keeps the full story.
                        ARCH_NAME="arm64 under Rosetta"
                        log_info "sysctl.proc_translated is 1, this shell runs under Rosetta on Apple silicon"
                        log_info "uname -m reported x86_64, the real CPU is arm64"
                        log_info "picking the native arm64 archive rather than the x86_64 one"
                    else
                        TARGET="x86_64-apple-darwin"
                    fi
                    ;;
                arm64 | aarch64) TARGET="aarch64-apple-darwin" ;;
                *)
                    TARGET=""
                    TARGET_REASON="no prebuilt archive is published for macOS $ARCH_NAME"
                    ;;
            esac
            ;;
        MINGW* | MSYS* | CYGWIN* | Windows_NT)
            err "this is the Unix installer. On Windows run: irm https://proa.so/install.ps1 | iex"
            ;;
        *)
            TARGET=""
            TARGET_REASON="$OS_NAME is not one of the operating systems release archives are built for"
            ;;
    esac

    if [ -n "$TARGET" ]; then
        log_info "target triple: $TARGET"
    else
        log_info "target triple: none, $TARGET_REASON"
    fi
}

# ---------------------------------------------------------------------------
# Downloading
# ---------------------------------------------------------------------------

detect_downloader() {
    if check_cmd curl; then
        DOWNLOADER="curl"
        log_info "downloader: curl"
        log_debug "curl --version: $(first_line "$( (curl --version) 2>&1 || true )")"
    elif check_cmd wget; then
        DOWNLOADER="wget"
        log_info "downloader: wget (curl was not found)"
        log_debug "wget --version: $(first_line "$( (wget --version) 2>&1 || true )")"
        # Not every wget is GNU wget. BusyBox builds accept neither
        # --https-only nor --secure-protocol, so probe for the capability
        # instead of assuming it. The check is deferred to download_to because
        # a source build that skips the optional Tailwind CLI downloads
        # nothing.
        if (wget --help) 2>&1 | grep -q -- '--https-only'; then
            WGET_STRICT_TLS="yes"
            log_debug "this wget supports --https-only"
        else
            log_warn "this wget does not support --https-only, it is probably the BusyBox build"
        fi
        # --show-progress arrived in wget 1.16 and is the only way to keep the
        # progress bar while --quiet suppresses everything else. It is probed
        # for the same reason as --https-only, and a wget without it simply
        # downloads silently: a missing progress bar is never an error.
        if (wget --help) 2>&1 | grep -q -- '--show-progress'; then
            WGET_PROGRESS="yes"
            log_debug "this wget supports --show-progress"
        else
            log_debug "this wget has no --show-progress, downloads will be silent"
        fi
    else
        err "neither curl nor wget is available, and one of them is needed to download the release"
    fi
}

# Prints the byte size of $1, or returns non-zero. Used only to fill in the
# bytes-received field of a log line, so a missing wc costs nothing.
file_size_of() {
    local out

    check_cmd wc || return 1
    out=$(wc -c < "$1" 2> /dev/null) || return 1
    # BSD wc pads with leading spaces.
    out="${out#"${out%%[0-9]*}"}"
    case "$out" in
        "" | *[!0-9]*) return 1 ;;
    esac
    printf '%s' "$out"
}

# $1 url, $2 destination path, $3 any non-empty value to keep transport errors
# off the screen, $4 any non-empty value to ask for a progress bar. Returns
# non-zero on any HTTP or transport error so callers can decide between falling
# back and failing.
#
# Callers that expect failure pass the quiet flag rather than redirecting this
# function's stderr. Redirecting it would also swallow the `err` below, and
# because `err` exits, the script would die with status 1 and no message at all.
#
# The progress bar is asked for by the archive download and by nothing else.
# latest.json and the sums file are a few hundred bytes each, so a bar for them
# would appear and vanish before it could be read, and two flickers on the way
# to one real download is exactly the noise this layout is trying to avoid. It
# is also suppressed off a terminal, where the carriage returns it is drawn
# with would land in whatever file the run was redirected into.
#
# Both the request and its outcome go to the log. A broken install is nearly
# always explained by one of these lines. LAST_DOWNLOAD_BYTES carries the size
# back to the caller, which prints it in human units while the log keeps the
# exact count.
download_to() {
    local url="$1"
    local dest="$2"
    local quiet="${3:-}"
    local progress="${4:-}"
    local rc=0
    local info=""
    local code=""
    local bytes=""

    LAST_DOWNLOAD_BYTES=""

    if [ -z "$IS_TTY" ]; then
        progress=""
    fi

    log_info "GET $url"
    log_debug "  destination: $dest"

    case "$DOWNLOADER" in
        curl)
            # --proto '=https' and --tlsv1.2 refuse a downgrade, --fail turns an
            # HTML error page into a non-zero exit, --location follows the
            # redirect chain. That chain can be two hops now: the download
            # bridge either streams the bytes itself or answers 302 to
            # github.com, which then redirects again to the asset CDN.
            # --proto '=https' applies to every hop, so a redirect cannot
            # downgrade the transfer. --show-error is what makes a silent curl
            # explain itself, so it is exactly what quiet drops.
            #
            # --write-out prints the status code and the transferred size on
            # stdout, which is otherwise unused here because the body goes to
            # --output. Capturing it keeps stdout clean for the caller and
            # gives the log the one field it cannot get any other way. curl
            # still prints the string when --fail rejects a 4xx, so a 404 is
            # logged as a 404 rather than as an anonymous failure.
            #
            # --progress-bar and --silent cancel each other out: --silent turns
            # the progress meter off, so the bar branch drops it and keeps
            # --show-error for the diagnostics --silent would have taken with
            # it. The bar is drawn on stderr, which leaves the --write-out
            # string on stdout for the substitution below.
            if [ -n "$progress" ]; then
                info=$(curl --proto '=https' --tlsv1.2 --location --fail --show-error \
                    --progress-bar \
                    --retry 3 --connect-timeout 20 \
                    --write-out '%{http_code} %{size_download}' \
                    --output "$dest" "$url") || rc=$?
            elif [ -n "$quiet" ]; then
                info=$(curl --proto '=https' --tlsv1.2 --location --fail --silent \
                    --retry 3 --connect-timeout 20 \
                    --write-out '%{http_code} %{size_download}' \
                    --output "$dest" "$url") || rc=$?
            else
                info=$(curl --proto '=https' --tlsv1.2 --location --fail --silent --show-error \
                    --retry 3 --connect-timeout 20 \
                    --write-out '%{http_code} %{size_download}' \
                    --output "$dest" "$url") || rc=$?
            fi

            code="${info%% *}"
            bytes="${info##* }"
            case "$code" in
                # curl writes 000 when it never got as far as a response,
                # which reads as a status code and is not one.
                000) code="none, the request never reached a server" ;;
                "" | *[!0-9]*) code="" ;;
            esac
            case "$bytes" in
                "" | *[!0-9]*) bytes="" ;;
            esac
            ;;
        wget)
            if [ -z "$WGET_STRICT_TLS" ]; then
                err "the only wget on this system cannot enforce HTTPS-only transfers (it is probably the BusyBox build). Install curl, for example 'apk add curl', and run this installer again."
            fi
            # --https-only is wget's equivalent of curl --proto '=https'. wget
            # already follows redirects and already exits non-zero on 4xx/5xx.
            #
            # There is no wget equivalent of --write-out, and --server-response
            # would put the whole header exchange on the console, so the status
            # code is simply unavailable on this path. The exit status and the
            # byte count still make the outcome clear.
            if [ -n "$progress" ] && [ -n "$WGET_PROGRESS" ]; then
                wget --https-only --secure-protocol=TLSv1_2 --tries=3 --timeout=20 \
                    --quiet --show-progress --output-document="$dest" "$url" || rc=$?
            else
                wget --https-only --secure-protocol=TLSv1_2 --tries=3 --timeout=20 \
                    --quiet --output-document="$dest" "$url" || rc=$?
            fi
            ;;
        *)
            err "no downloader selected, this is an installer bug"
            ;;
    esac

    if [ -z "$bytes" ] && [ -f "$dest" ]; then
        bytes=$(file_size_of "$dest") || bytes=""
    fi
    LAST_DOWNLOAD_BYTES="$bytes"

    if [ "$rc" -eq 0 ]; then
        log_info "  -> ok, HTTP ${code:-unknown to $DOWNLOADER}, ${bytes:-unknown} bytes"
        return 0
    fi

    log_warn "  -> failed, $DOWNLOADER exit $rc, HTTP ${code:-unknown to $DOWNLOADER}, ${bytes:-0} bytes written"
    return "$rc"
}

# ---------------------------------------------------------------------------
# Temporary directory
# ---------------------------------------------------------------------------

cleanup() {
    if [ -n "${TMP_DIR:-}" ] && [ -d "$TMP_DIR" ]; then
        rm -rf "$TMP_DIR"
    fi
}

make_tmp_dir() {
    need_cmd mktemp "used to create a private directory for the download"
    # The bare form covers GNU coreutils; the -t form covers the BSD mktemp on
    # macOS, which wants a template.
    TMP_DIR=$(mktemp -d 2> /dev/null || mktemp -d -t proa-install)
    [ -n "$TMP_DIR" ] && [ -d "$TMP_DIR" ] || err "could not create a temporary directory"
    log_info "temporary directory: $TMP_DIR (removed when this script exits)"
}

# The trap deletes TMP_DIR on every exit path, so a log written inside it would
# disappear at exactly the moment it became useful, and the run would look like
# it had succeeded. The default path is in /tmp and mktemp always returns a
# fresh subdirectory, so this can only trigger when PROA_LOG names a path
# inside it. The comparison is textual: a relative or symlinked PROA_LOG could
# still slip past, which is why the check is paired with never defaulting
# anywhere near TMP_DIR.
check_log_outside_tmp() {
    [ -n "$LOG_ENABLED" ] || return 0
    [ -n "${TMP_DIR:-}" ] || return 0

    case "$LOG_FILE" in
        "$TMP_DIR" | "$TMP_DIR"/*)
            warn "the log path $LOG_FILE is inside the temporary directory this script deletes on exit"
            if [ -n "$DEFAULT_LOG_FILE" ] && open_log "$DEFAULT_LOG_FILE"; then
                say "Writing the log to $DEFAULT_LOG_FILE instead."
                LOG_FILE="$DEFAULT_LOG_FILE"
                LOG_SOURCE="relocated out of the temporary directory"
                # This file starts mid-run, so it repeats enough of the header
                # to stand on its own in a bug report.
                log_info "=== proa installer ==="
                log_info "script: install.sh (revision $INSTALLER_REVISION)"
                log_info "log relocated here because the requested path was inside $TMP_DIR"
                log_inputs
            else
                LOG_ENABLED=""
                LOG_FILE=""
                warn "logging is off for this run rather than writing a log that deletes itself"
            fi
            ;;
        *)
            log_debug "log file is outside the temporary directory, it will survive cleanup"
            ;;
    esac
}

# ---------------------------------------------------------------------------
# Version comparison
# ---------------------------------------------------------------------------

# Numeric, component-by-component compare: returns 0 when $1 >= $2.
#
# sort -V is deliberately not used: busybox and older macOS do not have it. A
# string compare is also wrong, because "1.100.0" < "1.93.0" and "1.9.0" >
# "1.93.0" lexically, and both of those answers are backwards.
version_ge() {
    local left="$1"
    local right="$2"
    local lhs
    local rhs
    local i=0

    while [ "$i" -lt 4 ]; do
        lhs="${left%%.*}"
        rhs="${right%%.*}"

        # Trim any suffix such as the "-nightly" in 1.94.0-nightly, then treat
        # a missing or non-numeric component as zero so "1.93" == "1.93.0".
        lhs="${lhs%%[!0-9]*}"
        rhs="${rhs%%[!0-9]*}"
        [ -n "$lhs" ] || lhs=0
        [ -n "$rhs" ] || rhs=0

        if [ "$lhs" -gt "$rhs" ]; then
            return 0
        fi
        if [ "$lhs" -lt "$rhs" ]; then
            return 1
        fi

        case "$left" in
            *.*) left="${left#*.}" ;;
            *) left="0" ;;
        esac
        case "$right" in
            *.*) right="${right#*.}" ;;
            *) right="0" ;;
        esac
        i=$((i + 1))
    done

    # All compared components are equal.
    return 0
}

# Prints the bare version out of `rustc --version` ("rustc 1.93.0 (hash date)").
rustc_semver() {
    local line
    line=$(rustc --version 2> /dev/null) || return 1
    line="${line#rustc }"
    line="${line%% *}"

    # Anything that is not shaped like a version is reported as unparseable, not
    # returned. version_ge reads a non-numeric component as zero, so handing it
    # a shim's unexpected output would produce "Rust <junk> is older than the
    # minimum supported version", which sends the user off to fix a toolchain
    # that is fine.
    case "$line" in
        [0-9]*.[0-9]*) ;;
        *) return 1 ;;
    esac

    printf '%s' "$line"
}

# ---------------------------------------------------------------------------
# Rust preflight
# ---------------------------------------------------------------------------

rustup_command() {
    printf "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
}

# The full output of both tools, so a bug report shows the toolchain that was
# actually on PATH rather than the one the user believes is installed.
log_toolchain() {
    local out

    if check_cmd rustc; then
        if out=$( (rustc --version) 2>&1 ); then
            log_info "rustc --version: $out"
        else
            log_info "rustc --version failed: $out"
        fi
        log_debug "rustc path: $(command -v rustc)"
    else
        log_info "rustc: not found on PATH"
    fi

    if check_cmd cargo; then
        if out=$( (cargo --version) 2>&1 ); then
            log_info "cargo --version: $out"
        else
            log_info "cargo --version failed: $out"
        fi
        log_debug "cargo path: $(command -v cargo)"
    else
        log_info "cargo: not found on PATH"
    fi
}

# Proa compiles templates with cargo, so a project cannot be built without a
# toolchain even though the CLI itself is a prebuilt binary.
#
# The preflight is split in two because the headline block is printed before
# any of the work starts, and the toolchain row belongs in that block. probe
# only looks and writes to the log; ensure is the half that can prompt, install
# and warn, and it runs after the block is on screen.
#
# RUST_STATE is what probe_rust learned: ok, old, unreadable or missing.
probe_rust() {
    local found_version

    log_toolchain

    if check_cmd cargo && check_cmd rustc; then
        if found_version=$(rustc_semver); then
            RUST_VERSION="$found_version"
            if version_ge "$found_version" "$MSRV"; then
                RUST_STATE="ok"
                FACT_TOOLCHAIN="Rust $found_version (minimum $MSRV)"
                log_info "MSRV check: $found_version >= $MSRV, ok"
            else
                RUST_STATE="old"
                FACT_TOOLCHAIN="Rust $found_version (older than the minimum $MSRV)"
                log_warn "MSRV check: $found_version < $MSRV, too old"
            fi
        else
            RUST_STATE="unreadable"
            FACT_TOOLCHAIN="Rust installed, version not readable"
            log_warn "MSRV check: skipped, 'rustc --version' output could not be parsed"
        fi
        return 0
    fi

    RUST_STATE="missing"
    FACT_TOOLCHAIN="not found"
    log_info "no usable Rust toolchain: cargo or rustc is missing from PATH"
}

ensure_rust() {
    case "$RUST_STATE" in
        ok)
            return 0
            ;;
        old)
            warn "Rust $RUST_VERSION is older than the minimum supported version $MSRV."
            detail "The CLI will still install, but building a Proa project will fail."
            detail "Update with: rustup update stable"
            blank
            return 0
            ;;
        unreadable)
            warn "could not parse the output of 'rustc --version', skipping the version check"
            blank
            return 0
            ;;
    esac

    # Rust is a hard requirement rather than something this installer will
    # provision. Proa builds your project with cargo, so a CLI installed
    # without a toolchain cannot do the one thing it exists for, and adding a
    # toolchain to someone's machine is a decision they should make
    # deliberately instead of inheriting from a piped shell script.
    log_info "decision: no Rust toolchain found, stopping"
    # Worth the length: this is the one failure a first-time user is most
    # likely to hit, and the fix is not guessable from "cargo: not found".
    # It names the cause, the reason Rust is not optional here, both ways to
    # install it, and the PATH gotcha that makes a fresh rustup look broken.
    #
    # Every line here is written at one of the two columns the rest of the
    # script uses: prose sits flush with the label blocks (fail_line adds the
    # two-space indent), and anything meant to be clicked or pasted is indented
    # two more, which is exactly where cmd_line puts a command. Nothing invents
    # a third column, because a hand-built label/value pair inside a message
    # cannot line up with the ones row() and link() produce.
    err "Rust is required, and no toolchain was found on this machine.\
${NL}${NL}Proa compiles your templates into your binary at build time, so a Proa\
${NL}project is an ordinary Rust crate that cargo builds. Without cargo and\
${NL}rustc there is nothing for the CLI to drive, so installing it now would\
${NL}leave you with a command that cannot build your project.\
${NL}${NL}Install Rust ($MSRV or newer), then run this installer again.\
${NL}${NL}Getting started guide:\
${NL}  https://rust-lang.org/learn/get-started/\
${NL}${NL}Or install it directly:\
${NL}  $(rustup_command)\
${NL}${NL}If you just installed Rust, cargo is not on this shell's PATH yet.\
${NL}Open a new terminal, or load it into this one:\
${NL}  . \"\$HOME/.cargo/env\""
}

# ---------------------------------------------------------------------------
# Install directory and existing installs
# ---------------------------------------------------------------------------

resolve_bin_dir() {
    if [ -n "$INSTALL_DIR" ]; then
        BIN_DIR="$INSTALL_DIR"
    else
        BIN_DIR="$PROA_ROOT/bin"
    fi

    # Strip a trailing slash so PATH comparisons and profile lines are stable.
    case "$BIN_DIR" in
        */) BIN_DIR="${BIN_DIR%/}" ;;
    esac
    [ -n "$BIN_DIR" ] || err "the install directory resolved to an empty path"

    if [ -n "$DIR_WAS_EXPLICIT" ]; then
        log_info "install directory: $BIN_DIR (named explicitly)"
    else
        log_info "install directory: $BIN_DIR (default, from PROA_HOME $PROA_ROOT)"
    fi
}

record_previous_version() {
    local out
    if [ -x "$BIN_DIR/proa" ]; then
        if out=$("$BIN_DIR/proa" --version 2> /dev/null); then
            PREVIOUS_VERSION="${out#proa }"
            log_info "existing install at $BIN_DIR/proa reports version $PREVIOUS_VERSION"
        else
            PREVIOUS_VERSION="unknown"
            log_warn "existing install at $BIN_DIR/proa did not answer --version"
        fi
    else
        log_info "no existing proa binary at $BIN_DIR/proa, this is a fresh install"
    fi
}

# ---------------------------------------------------------------------------
# Release resolution
# ---------------------------------------------------------------------------

# Fills VERSION from <download base>/latest.json. Returns non-zero when there is
# nothing to install, which the caller turns into a source build rather than an
# error.
#
# This deliberately does not ask api.github.com/repos/<repo>/releases/latest.
# That endpoint answers with the newest release of ANY crate in the monorepo, so
# the moment proa_core or rsjs is tagged it would hand back a version that has
# no proa-cli archives at all and every download would 404. The bridge filters
# on the proa-cli-v tag prefix, which makes reading latest.json correct by
# construction rather than correct until the next unrelated tag.
resolve_latest_version() {
    local url="$DOWNLOAD_BASE/latest.json"
    local json="$TMP_DIR/latest.json"
    local version

    need_cmd sed "used to read the version out of the latest.json response"
    log_info "resolving the latest release from $url"

    # The downloader is asked to be quiet because a 404 from this endpoint is an
    # expected answer ("no proa-cli release published yet") rather than a
    # failure, and the caller prints a message that covers every reason this
    # comes back empty. A 503 (GitHub unreachable or rate limited from the
    # bridge's side) lands here the same way. Note this is the quiet flag and
    # not a `2> /dev/null` on the call: the redirect would also hide
    # download_to's own fatal misconfiguration error, which exits, and the
    # script would then die without printing a word.
    if ! download_to "$url" "$json" quiet; then
        log_warn "the latest.json request failed, so no release version is available"
        return 1
    fi

    # The payload is {"version":..., "tag":..., "assets":[...]}. The server may
    # send it compact or pretty-printed, so neither layout is assumed.
    #
    # The key is anchored on the '{' , ',' or whitespace in front of it rather
    # than matched loosely, because "assets" is an array of strings and a
    # pattern that can start anywhere is one asset name away from capturing the
    # wrong thing. The capture is [^"]* for the same reason: it cannot run past
    # the closing quote into the next field. The second expression is the
    # pretty-printed case where "version" opens the line with no indentation, so
    # there is no preceding character for the first one to anchor on.
    #
    # Whatever comes back is still only a hint. It is checked against
    # is_release_version below, the archive it names has to exist, and that
    # archive has to match SHA256SUMS before anything is installed.
    version=$(sed -n \
        -e 's/.*[{,[:space:]]"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
        -e 's/^"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
        "$json" | head -n 1)
    if [ -z "$version" ]; then
        log_warn "the latest.json response carried no version field"
        return 1
    fi
    log_info "latest.json reported version: $version"

    # The bridge publishes a bare version, but a mirror may well answer with the
    # tag instead. Both spellings are accepted, exactly as --version does.
    version="${version#proa-cli-}"
    version="${version#v}"

    # The version comes off the network and is about to become part of a URL and
    # of a file name under TMP_DIR, so it gets the same check as the --version
    # flag. A version this does not recognise is treated as "no usable release"
    # and falls through to the source build.
    if ! is_release_version "$version"; then
        log_warn "version '$version' is not shaped like a release version, ignoring it"
        return 1
    fi

    VERSION="$version"
    log_info "resolved version: $VERSION"
    return 0
}

# ---------------------------------------------------------------------------
# Checksums
# ---------------------------------------------------------------------------

# Picks the hash tool and records it in SHA_TOOL. This is a separate step
# because sha256_of is always called inside a command substitution, and an
# assignment made in that subshell would be thrown away with it, leaving the
# log unable to name the tool that produced the hash.
select_sha_tool() {
    if check_cmd sha256sum; then
        SHA_TOOL="sha256sum"
    elif check_cmd shasum; then
        SHA_TOOL="shasum -a 256"
    elif check_cmd openssl; then
        SHA_TOOL="openssl dgst -sha256"
    else
        SHA_TOOL=""
    fi
}

# Prints the lowercase sha256 of $1. No pipelines, because POSIX sh has no
# pipefail and a failing hash tool would otherwise be masked by the filter.
sha256_of() {
    local file="$1"
    local out

    [ -n "$SHA_TOOL" ] || select_sha_tool

    case "$SHA_TOOL" in
        sha256sum)
            out=$(sha256sum "$file") || return 1
            ;;
        "shasum -a 256")
            out=$(shasum -a 256 "$file") || return 1
            ;;
        "openssl dgst -sha256")
            out=$(openssl dgst -sha256 "$file") || return 1
            # openssl prints "SHA256(file)= <hash>", so take the last field.
            out="${out##* }"
            printf '%s' "$out"
            return 0
            ;;
        *)
            return 1
            ;;
    esac

    # Both sha256sum and shasum print "<hash>  <name>".
    printf '%s' "${out%% *}"
}

# $1 SHA256SUMS path, $2 file name. Prints the expected hash, empty if absent.
expected_sha_for() {
    local sums="$1"
    local wanted="$2"
    local hash
    local name
    local cr
    cr=$(printf '\r')

    # A `while read` fed by a redirect runs in this shell, so the result is
    # readable afterwards without a subshell or a pipeline.
    #
    # `|| [ -n "$hash" ]` keeps the last line of a file that has no trailing
    # newline. read sets the variables and then returns non-zero at EOF, so the
    # plain form silently drops that entry, and a one-entry sums file would look
    # like it had no entry for the archive at all.
    while read -r hash name || [ -n "$hash" ]; do
        # A sums file generated on Windows carries a trailing CR that would
        # never compare equal, and sha256sum writes "*name" for binary-mode
        # entries. Strip the CR first, since it sits at the end of the line.
        name="${name%"$cr"}"
        name="${name#\*}"
        if [ "$name" = "$wanted" ]; then
            printf '%s' "$hash"
            return 0
        fi
    done < "$sums"

    return 0
}

verify_checksum() {
    local archive_path="$1"
    local sums_path="$2"
    local archive_name="$3"
    local expected
    local actual

    expected=$(expected_sha_for "$sums_path" "$archive_name")
    if [ -z "$expected" ]; then
        log_write ERROR "checksum: no entry for $archive_name in $sums_path"
        err "$archive_name has no entry in the checksum file, refusing to install an unverified binary"
    fi
    log_info "checksum expected: $expected ($archive_name)"

    select_sha_tool
    if ! actual=$(sha256_of "$archive_path"); then
        err "no sha256 tool found (sha256sum, shasum or openssl), refusing to install an unverified binary"
    fi
    log_info "checksum computed: $actual (via ${SHA_TOOL:-unknown tool})"

    if [ "$expected" != "$actual" ]; then
        log_write ERROR "checksum verdict: MISMATCH"
        # Both hashes in full, one per line, under each other. This is the one
        # failure a reader is expected to compare character by character, and
        # truncating either of them or running them together inside a sentence
        # makes that impossible.
        err "checksum mismatch for $archive_name${NL}expected  $expected${NL}received  $actual${NL}${NL}The download is corrupt or has been tampered with. No unverified files were installed."
    fi

    log_info "checksum verdict: match"
    step_ok "checksum" "sha256 verified${CHECKSUM_SIZE_SUFFIX}"
}

# ---------------------------------------------------------------------------
# Optional Tailwind CSS CLI
# ---------------------------------------------------------------------------

# Selects the official standalone asset and its release-published SHA-256 for
# this machine. Unlike Proa's own release archives, Tailwind publishes musl
# binaries, so Linux musl can take this path even when Proa builds from source.
select_tailwind_asset() {
    case "$OS_NAME:$TARGET:$UNAME_M:$LIBC_KIND" in
        Darwin:aarch64-apple-darwin:*)
            TAILWIND_ASSET="tailwindcss-macos-arm64"
            TAILWIND_SHA256="cdf646702987a743464dff4d9c60fd4480d1c1e73dd819a9a67f1078815dce9d"
            ;;
        Darwin:x86_64-apple-darwin:*)
            TAILWIND_ASSET="tailwindcss-macos-x64"
            TAILWIND_SHA256="7922e0953f2110c05976e3bf58f14e643d90427575e766b7d433f5f80cbee7e1"
            ;;
        Linux:*:x86_64:glibc | Linux:*:amd64:glibc)
            TAILWIND_ASSET="tailwindcss-linux-x64"
            TAILWIND_SHA256="dc61b3ac6b8c9ca874c0cc4c57b2409791a64c5540404ca5f5367360babc313a"
            ;;
        Linux:*:aarch64:glibc | Linux:*:arm64:glibc)
            TAILWIND_ASSET="tailwindcss-linux-arm64"
            TAILWIND_SHA256="55fd0b241214eff3de1e8ee4f22796662f2d2e7a49bcfca7477cfd0bac398195"
            ;;
        Linux:*:x86_64:musl | Linux:*:amd64:musl)
            TAILWIND_ASSET="tailwindcss-linux-x64-musl"
            TAILWIND_SHA256="a04d34ceacc8f52cbe8920ad846cdeb61d3d0021dba32db0d1f77c9d9fad7a6c"
            ;;
        Linux:*:aarch64:musl | Linux:*:arm64:musl)
            TAILWIND_ASSET="tailwindcss-linux-arm64-musl"
            TAILWIND_SHA256="71ea4be79c9de9827545682df3e040053fb535d37c71ed2cfdedf9385a0868e0"
            ;;
        *)
            TAILWIND_ASSET=""
            TAILWIND_SHA256=""
            return 1
            ;;
    esac
    return 0
}

# Returns success when the first tailwindcss on PATH is already the exact
# pinned version. `--help` works across the standalone v4 binaries and prints
# the version without requiring an input stylesheet.
tailwind_is_current() {
    local bin
    local out

    check_cmd tailwindcss || return 1
    bin=$(command -v tailwindcss)
    out=$(tailwindcss --help 2>&1 || true)
    case "$out" in
        *"v$TAILWIND_VERSION"*)
            log_info "existing Tailwind CLI at $bin is the pinned v$TAILWIND_VERSION"
            return 0
            ;;
        *)
            log_info "existing Tailwind CLI at $bin is not the pinned v$TAILWIND_VERSION"
            return 1
            ;;
    esac
}

install_tailwind() {
    local url
    local asset_path
    local sums_path="$TMP_DIR/tailwindcss-v$TAILWIND_VERSION-SHA256SUMS"
    local out
    local shadow

    if ! select_tailwind_asset; then
        if [ -n "$TAILWIND_ONLY" ]; then
            err "Tailwind CSS does not publish a standalone v$TAILWIND_VERSION binary for $OS_NAME $UNAME_M"
        fi
        warn "Tailwind CSS does not publish a standalone v$TAILWIND_VERSION binary for $OS_NAME $UNAME_M; install it with your package manager instead"
        return 0
    fi

    url="$TAILWIND_RELEASE_BASE/$TAILWIND_ASSET"
    asset_path="$TMP_DIR/$TAILWIND_ASSET"
    CHECKSUM_SIZE_SUFFIX=""

    log_section "tailwind"
    log_info "Tailwind version: $TAILWIND_VERSION (pinned by installer revision $INSTALLER_REVISION)"
    log_info "Tailwind asset: $TAILWIND_ASSET"
    step_run "tailwind" "downloading v$TAILWIND_VERSION ($TAILWIND_ASSET)"
    if ! download_to "$url" "$asset_path" "" progress; then
        err "could not download the Tailwind CSS CLI v$TAILWIND_VERSION from its official release"
    fi
    if [ -n "$LAST_DOWNLOAD_BYTES" ]; then
        CHECKSUM_SIZE_SUFFIX=" ($(format_bytes "$LAST_DOWNLOAD_BYTES"))"
    fi

    # Reuse the same checksum verifier as the Proa archive, but seed it with a
    # hash pinned in this script rather than trusting a checksum fetched beside
    # the binary it is meant to authenticate.
    printf '%s  %s\n' "$TAILWIND_SHA256" "$TAILWIND_ASSET" > "$sums_path" ||
        err "could not prepare the pinned Tailwind checksum"
    verify_checksum "$asset_path" "$sums_path" "$TAILWIND_ASSET"

    mkdir -p "$BIN_DIR" || err "could not create $BIN_DIR"
    install_file "$asset_path" "$BIN_DIR/tailwindcss"
    if ! out=$("$BIN_DIR/tailwindcss" --help 2>&1); then
        err "$BIN_DIR/tailwindcss was installed but does not run. Output: $out"
    fi
    case "$out" in
        *"v$TAILWIND_VERSION"*) ;;
        *) err "$BIN_DIR/tailwindcss was installed but did not report v$TAILWIND_VERSION" ;;
    esac

    step_ok "tailwind" "v$TAILWIND_VERSION installed into $(tilde_path "$BIN_DIR")"
    log_info "verified $BIN_DIR/tailwindcss runs and reports v$TAILWIND_VERSION"

    if check_cmd tailwindcss; then
        shadow=$(command -v tailwindcss)
        if [ "$shadow" != "$BIN_DIR/tailwindcss" ]; then
            warn "another tailwindcss is earlier on your PATH at $shadow and will win until you reorder PATH"
        fi
    fi
}

maybe_install_tailwind() {
    case "$TAILWIND_CHOICE" in
        skip)
            log_info "Tailwind install skipped by explicit configuration"
            step_run "tailwind" "skipped by configuration"
            return 0
            ;;
        ask)
            if tailwind_is_current; then
                step_ok "tailwind" "v$TAILWIND_VERSION is already on PATH"
                return 0
            fi
            if [ -n "$ASSUME_YES" ]; then
                log_info "Tailwind prompt answered yes by --yes"
            elif is_interactive; then
                if ! prompt_yes_no "Install Tailwind CSS CLI v$TAILWIND_VERSION too?"; then
                    step_run "tailwind" "skipped"
                    return 0
                fi
            else
                log_info "Tailwind prompt skipped because this run is non-interactive"
                step_run "tailwind" "skipped (use --with-tailwind to install)"
                return 0
            fi
            ;;
    esac

    install_tailwind
}

# ---------------------------------------------------------------------------
# Installing files
# ---------------------------------------------------------------------------

# macOS marks downloaded files as quarantined, which makes Gatekeeper refuse
# the binary. Removing the attribute is best effort: it is absent on Linux and
# absent on macOS files that were never quarantined.
strip_quarantine() {
    local file="$1"
    if [ "$OS_NAME" = "Darwin" ] && check_cmd xattr; then
        xattr -d com.apple.quarantine "$file" 2> /dev/null || true
        log_debug "cleared com.apple.quarantine on $file (best effort)"
    fi
}

# $1 source file, $2 destination file.
install_file() {
    local src="$1"
    local dest="$2"

    [ -f "$src" ] || err "expected $src in the archive but it is missing"

    # Removing first avoids ETXTBSY when the destination is a binary that is
    # currently running, and keeps the replacement atomic enough in practice.
    rm -f "$dest" 2> /dev/null || true
    cp "$src" "$dest" || err "could not write $dest. Check the directory permissions, or pass --to with a writable directory."
    chmod 755 "$dest" || err "could not make $dest executable"
    strip_quarantine "$dest"
    log_info "installed $src -> $dest (mode 755)"
}

# ---------------------------------------------------------------------------
# PATH handling
# ---------------------------------------------------------------------------

path_contains() {
    case ":${PATH:-}:" in
        *":$1:"*) return 0 ;;
        *) return 1 ;;
    esac
}

# $1 the comma-separated list so far, $2 a path to append by basename. Used to
# build ".bashrc, .profile" for the console while the log keeps full paths.
join_basename() {
    printf '%s%s' "${1:+$1, }" "${2##*/}"
}

# Appends the export line to a profile unless it is already mentioned there.
# $1 profile path, $2 line to append.
append_to_profile() {
    local profile="$1"
    local line="$2"

    # Grep before writing so a second run of the installer is a no-op. Both the
    # rendered line and the raw directory are checked, because an earlier run
    # (or the user) may have written either spelling.
    if grep -Fq -- "$line" "$profile" 2> /dev/null; then
        log_debug "$profile already contains the exact export line"
        return 1
    fi
    if grep -Fq -- "$BIN_DIR" "$profile" 2> /dev/null; then
        log_debug "$profile already mentions $BIN_DIR in some other form"
        return 1
    fi

    printf '\n# Added by the Proa installer\n%s\n' "$line" >> "$profile" ||
        err "could not write to $profile"
    return 0
}

update_path() {
    local display="$BIN_DIR"
    local posix_line
    local fish_line
    local fish_config="$HOME/.config/fish/config.fish"
    local profile
    local updated=""
    local names=""
    local found_profile=""

    if path_contains "$BIN_DIR"; then
        log_info "PATH already contains $BIN_DIR, no profile is examined"
        PATH_STATE="already"
        step_ok "path" "$(tilde_path "$BIN_DIR") is already on PATH"
        return 0
    fi
    log_info "PATH does not contain $BIN_DIR, examining shell profiles"

    # Write $HOME rather than the expanded path when possible, so the profile
    # stays portable across machines and users.
    case "$BIN_DIR" in
        "$HOME"/*) display="\$HOME${BIN_DIR#"$HOME"}" ;;
    esac
    posix_line="export PATH=\"$display:\$PATH\""
    fish_line="set -gx PATH \"$BIN_DIR\" \$PATH"
    # The $HOME form is what the closing block offers to paste, so a reader who
    # copies it into a second machine copies something portable.
    PATH_EXPORT_LINE="$posix_line"
    log_debug "posix profile line: $posix_line"
    log_debug "fish profile line: $fish_line"

    if [ -n "$NO_MODIFY_PATH" ]; then
        log_info "no profile is modified: --no-modify-path (or PROA_NO_MODIFY_PATH) is in effect"
        log_info "$BIN_DIR is not on PATH and this run was asked not to change your profile"
        PATH_STATE="manual"
        step_run "path" "left alone (--no-modify-path)"
        return 0
    fi

    for profile in "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.zshrc" "$HOME/.profile"; do
        if [ -f "$profile" ]; then
            found_profile="yes"
            if append_to_profile "$profile" "$posix_line"; then
                updated="$updated $profile"
                names=$(join_basename "$names" "$profile")
                log_info "profile $profile: appended the PATH line"
            else
                log_info "profile $profile: already correct, left alone"
            fi
        else
            log_debug "profile $profile: does not exist"
        fi
    done

    if [ -f "$fish_config" ]; then
        found_profile="yes"
        if grep -Fq -- "$BIN_DIR" "$fish_config" 2> /dev/null; then
            log_info "profile $fish_config: already correct, left alone"
        else
            printf '\n# Added by the Proa installer\n%s\n' "$fish_line" >> "$fish_config" ||
                err "could not write to $fish_config"
            updated="$updated $fish_config"
            names=$(join_basename "$names" "$fish_config")
            log_info "profile $fish_config: appended the fish PATH line"
        fi
    else
        log_debug "profile $fish_config: does not exist"
    fi

    # A machine with no profile at all still needs somewhere for the line to
    # live, and ~/.profile is the one every POSIX login shell reads.
    if [ -z "$found_profile" ]; then
        log_info "no shell profile exists, creating $HOME/.profile"
        if append_to_profile "$HOME/.profile" "$posix_line"; then
            updated="$updated $HOME/.profile"
            names=$(join_basename "$names" "$HOME/.profile")
            log_info "profile $HOME/.profile: appended the PATH line"
        fi
    fi

    PATH_STATE="profile"
    if [ -n "$updated" ]; then
        # Basenames on screen, full paths in the log. Four home-relative paths
        # on one row wrap on any terminal, and every one of them was already
        # written to the log in full by the loops above.
        log_info "added $BIN_DIR to PATH in:$updated"
        step_ok "path" "added to $names"
    else
        step_ok "path" "already in your shell profile"
    fi
}

# ---------------------------------------------------------------------------
# Verification and next steps
# ---------------------------------------------------------------------------

verify_install() {
    local bin="$BIN_DIR/proa"
    local out
    local shadow

    [ -x "$bin" ] || err "installation finished but $bin is not executable"

    if ! out=$("$bin" --version 2>&1); then
        err "$bin was installed but does not run. Output: $out"
    fi
    log_info "verified $bin runs, '--version' says: $out"

    # Held rather than printed: the headline closes the run, below the step
    # rows and below whatever update_path has to say.
    if [ -n "$PREVIOUS_VERSION" ]; then
        READY_LINE="$out is ready, upgraded from $PREVIOUS_VERSION."
        log_info "upgraded: $PREVIOUS_VERSION -> ${out#proa }"
    else
        READY_LINE="$out is ready."
        log_info "installed: $out"
    fi

    if check_cmd proa; then
        shadow=$(command -v proa)
        log_info "first 'proa' on PATH resolves to $shadow"
        if [ "$shadow" != "$bin" ]; then
            warn "another proa is earlier on your PATH at $shadow and will win until you reorder PATH"
        fi
    else
        log_debug "no 'proa' on PATH yet in this shell, the profile line applies to new shells"
    fi
}

# The opening block: what this run decided about the machine, before it does
# anything to it. Everything it prints was worked out silently by the detection
# above, which is why the block comes out in one piece instead of a line at a
# time between network calls.
print_header() {
    local platform="$OS_NAME $ARCH_NAME"

    title "Proa CLI installer"
    blank

    if [ -n "$TARGET" ]; then
        platform="$platform ($TARGET)"
    else
        platform="$platform (no prebuilt archive)"
    fi

    fact "platform" "$platform"
    fact "toolchain" "$FACT_TOOLCHAIN"

    # Where the bytes come from, which is not the same question on the two
    # paths: a download names the host it downloads from, a source build names
    # the repository it compiles.
    if [ "$INSTALL_MODE" = "source" ]; then
        if source_uses_registry; then
            fact "source" "crates.io/proa-cli (built on this machine)"
        else
            fact "source" "github.com/$REPO (built on this machine)"
        fi
        if [ -n "$VERSION" ]; then fact "version" "$VERSION"; fi
    else
        fact "source" "$(display_url "$DOWNLOAD_BASE")"
        fact "version" "$VERSION"
    fi

    blank

    if [ -n "$MODE_NOTE" ]; then
        print_lines "$MODE_NOTE" say
        blank
    fi
}

print_tailwind_only_header() {
    title "Tailwind CSS CLI installer"
    blank
    fact "platform" "$OS_NAME $ARCH_NAME"
    fact "source" "github.com/tailwindlabs/tailwindcss"
    fact "version" "$TAILWIND_VERSION"
    blank
}

# The closing block. The Log line is not printed here: it is printed by the
# exit trap, which is the one place that runs on every path, and it lands
# directly under the Docs line because both are link rows.
print_summary() {
    blank
    title "$READY_LINE"
    blank

    case "$PATH_STATE" in
        profile)
            say "Start a new shell, or run this one:"
            cmd_line "$PATH_EXPORT_LINE"
            blank
            say "Then create a site:"
            ;;
        manual)
            say "Add this to your shell profile, or run it in this one:"
            cmd_line "$PATH_EXPORT_LINE"
            blank
            say "Then create a site:"
            ;;
        *)
            say "Create a site:"
            ;;
    esac

    cmd_line "proa new site hello-proa --template marketing --tailwind --yes"
    blank
    link "Docs" "https://proa.so/docs"
}

print_tailwind_only_summary() {
    blank
    title "tailwindcss v$TAILWIND_VERSION is ready."
    blank

    case "$PATH_STATE" in
        profile)
            say "Start a new shell, or run this one:"
            cmd_line "$PATH_EXPORT_LINE"
            ;;
        manual)
            say "Add this to your shell profile, or run it in this one:"
            cmd_line "$PATH_EXPORT_LINE"
            ;;
        *)
            say "Tailwind CSS CLI is available on PATH."
            ;;
    esac
}

# ---------------------------------------------------------------------------
# Install paths
# ---------------------------------------------------------------------------

# Released versions come from crates.io. An explicit unversioned source build
# or a custom repository uses Git; main snapshots always pin their full SHA.
source_uses_registry() {
    case "$VERSION" in
        *-main.*) return 1 ;;
    esac
    local normalized_repo
    normalized_repo=$(printf '%s' "$REPO" | tr '[:upper:]' '[:lower:]')
    case "$normalized_repo" in
        proa-labs/proa) ;;
        *) return 1 ;;
    esac
    [ -n "$VERSION" ] || [ -z "$BUILD_FROM_SOURCE" ]
}

install_from_source() {
    local cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"

    if ! check_cmd cargo; then
        err "building from source needs cargo. Install Rust first: $(rustup_command)"
    fi

    # cargo install always writes to cargo's own bin directory, so a requested
    # directory cannot be honoured on this path.
    if [ -n "$DIR_WAS_EXPLICIT" ] && [ "$BIN_DIR" != "$cargo_bin" ]; then
        warn "cargo installs into $cargo_bin, so the requested directory $BIN_DIR was not used"
    fi
    if [ "$BIN_DIR" != "$cargo_bin" ]; then
        # Any existing install that matters here is cargo's, so re-read the
        # previous version from there before the build overwrites it.
        log_info "install directory switched to $cargo_bin because cargo install decides its own destination"
        BIN_DIR="$cargo_bin"
        PREVIOUS_VERSION=""
        record_previous_version
    fi

    log_section "install"
    step_run "building" "proa-cli from source, this takes several minutes"
    blank

    # The clone itself goes through the git CLI when one is available
    # (CARGO_NET_GIT_FETCH_WITH_CLI). Cargo's built-in fetcher speaks its own
    # git and its own TLS: it ignores credential helpers, url.*.insteadOf
    # rewrites, http.sslCAInfo, and most proxy setups from ~/.gitconfig, all
    # of which the git binary honors. While this repository was private that
    # made anonymous-looking HTTPS clones fail outright, and users had to
    # export the variable by hand; behind corporate proxies and custom CAs it
    # still does. Setting it here makes the robust path the default. A caller
    # who exported a value keeps it, and without a git binary the built-in
    # fetcher is the only option anyway.
    set -- install --locked proa-cli
    if source_uses_registry; then
        set -- "$@" --registry crates-io
        if [ -n "$VERSION" ]; then set -- "$@" --version "=$VERSION"; fi
    else
        set -- "$@" --git "https://github.com/$REPO"
        case "$VERSION" in
            *-main.*)
                local revision="${VERSION#*-main.}"
                revision="${revision%%.*}"
                case "$revision" in
                    *[!0-9a-fA-F]*) err "main snapshot version must contain a full 40-character commit SHA" ;;
                esac
                [ "${#revision}" -eq 40 ] || err "main snapshot version must contain a full 40-character commit SHA"
                set -- "$@" --rev "$revision"
                ;;
            "") ;;
            *) set -- "$@" --tag "proa-cli-v$VERSION" ;;
        esac
    fi
    if source_uses_registry; then
        detail "Building proa-cli ${VERSION:-latest} from crates.io."
    else
        detail "Building proa-cli ${VERSION:-Git HEAD} from github.com/$REPO."
    fi
    log_info "running: cargo $*"
    local fetch_with_cli
    if check_cmd git; then
        fetch_with_cli="${CARGO_NET_GIT_FETCH_WITH_CLI:-true}"
    else
        fetch_with_cli="${CARGO_NET_GIT_FETCH_WITH_CLI:-false}"
        log_info "git is not installed; cargo falls back to its built-in git fetcher"
    fi
    log_debug "CARGO_NET_GIT_FETCH_WITH_CLI=$fetch_with_cli"
    CARGO_NET_GIT_FETCH_WITH_CLI="$fetch_with_cli" \
        cargo "$@" ||
        err "cargo install failed. The output above has the compiler error."
    log_info "cargo install finished, binaries are in $cargo_bin"

    blank
    step_ok "installed" "proa, cargo-proa into $(tilde_path "$cargo_bin")"

    verify_install
    maybe_install_tailwind
    update_path
    print_summary
}

# Writes the archive listing to the log. Best effort: a tar that cannot list is
# not a reason to stop, since the extraction itself is checked right after.
log_archive_contents() {
    local path="$1"
    local listing="$TMP_DIR/archive-listing.txt"
    local entry=""

    if [ -z "$LOG_ENABLED" ] && [ -z "$VERBOSE" ]; then
        return 0
    fi

    tar -tzf "$path" > "$listing" 2> /dev/null || return 0
    while read -r entry || [ -n "$entry" ]; do
        [ -n "$entry" ] || continue
        log_debug "  archive entry: $entry"
    done < "$listing"
}

install_prebuilt() {
    # These two names are the contract with the release workflow and with the
    # bridge's asset whitelist. They are unchanged by the move to proa.so:
    # only the base in front of them moved.
    local archive="proa-cli-$VERSION-$TARGET.tar.gz"
    local sums="proa-cli-$VERSION-SHA256SUMS"
    local base="$DOWNLOAD_BASE/$VERSION"
    local archive_path="$TMP_DIR/$archive"
    local sums_path="$TMP_DIR/$sums"
    local unpacked="$TMP_DIR/proa-cli-$VERSION-$TARGET"
    local name

    need_cmd tar "used to unpack the release archive"

    log_section "download"
    log_info "download base URL: $base"
    log_info "archive: $archive"
    log_info "checksum file: $sums"

    # Announced before the transfer so curl's progress bar draws underneath a row
    # that already says what is being fetched. Printing it afterwards put a bar
    # on screen for something the reader had not been told about yet. The size is
    # not known until the bytes land, so it rides on the checksum row instead.
    step_run "downloading" "$archive"
    if ! download_to "$base/$archive" "$archive_path" "" progress; then
        step_fail "download" "no archive at $base/$archive"
        if [ -n "$PREBUILT_ONLY" ]; then
            log_write ERROR "decision: no prebuilt archive for $TARGET and source fallback is disabled"
            err "no prebuilt proa-cli $VERSION archive exists for $TARGET at $(display_url "$base"); remove --prebuilt-only to allow a source build"
        fi
        log_warn "decision: no prebuilt archive for $TARGET at that URL, falling back to a source build"
        detail "Falling back to a source build."
        install_from_source
        return 0
    fi
    if [ -n "$LAST_DOWNLOAD_BYTES" ]; then
        CHECKSUM_SIZE_SUFFIX=" ($(format_bytes "$LAST_DOWNLOAD_BYTES"))"
    fi

    if ! download_to "$base/$sums" "$sums_path"; then
        err "downloaded $archive but its checksum file $sums is missing, refusing to install an unverified binary"
    fi

    verify_checksum "$archive_path" "$sums_path" "$archive"

    log_archive_contents "$archive_path"
    tar -xzf "$archive_path" -C "$TMP_DIR" || err "could not unpack $archive"
    log_info "unpacked $archive into $TMP_DIR"

    # The archive holds a single top-level proa-cli-<version>-<target>/
    # directory. Older or hand-built archives may be flat, so accept that too.
    if [ ! -d "$unpacked" ]; then
        log_info "no $unpacked directory in the archive, treating it as a flat layout"
        unpacked="$TMP_DIR"
    fi

    # Both binaries are checked before either is written. install_file also
    # checks, but it checks too late: a truncated archive would let proa land
    # and then abort on cargo-proa, leaving a half install that was never put on
    # PATH, or on an upgrade a new proa beside the previous cargo-proa.
    for name in proa cargo-proa; do
        [ -f "$unpacked/$name" ] ||
            err "$archive does not contain $name, refusing to install a partial toolchain. Nothing was written."
    done
    log_info "archive contains both proa and cargo-proa"

    log_section "install"
    log_info "installing into $BIN_DIR"
    mkdir -p "$BIN_DIR" || err "could not create $BIN_DIR"
    install_file "$unpacked/proa" "$BIN_DIR/proa"
    install_file "$unpacked/cargo-proa" "$BIN_DIR/cargo-proa"
    step_ok "installed" "proa, cargo-proa into $(tilde_path "$BIN_DIR")"

    verify_install
    maybe_install_tailwind
    update_path
    print_summary
}

# Decides between the prebuilt download and the source build, and resolves the
# version the download would use. Silent apart from the log: it runs before the
# headline block so that block can state the version and the source it settled
# on, rather than announcing a plan and then contradicting it two lines later.
# A reason worth showing the reader is left in MODE_NOTE for print_header.
choose_install_mode() {
    # No section marker here. Everything this decides, the version lookup
    # included, is a fact about what is going to be installed, so it belongs
    # with the platform facts. The download section opens when bytes that
    # matter start moving, which is install_prebuilt.
    if [ -n "$BUILD_FROM_SOURCE" ]; then
        INSTALL_MODE="source"
        log_info "decision: building from source because --build-from-source was requested"
        return 0
    fi

    if [ -z "$TARGET" ]; then
        if [ -n "$PREBUILT_ONLY" ]; then
            err "no prebuilt proa-cli archive is published for $OS_NAME $ARCH_NAME; remove --prebuilt-only to allow a source build"
        fi
        if [ -z "$VERSION" ]; then resolve_latest_version || :; fi
        INSTALL_MODE="source"
        log_info "decision: building from source because there is no prebuilt asset for this machine"
        MODE_NOTE="There is no prebuilt proa-cli archive for $OS_NAME $ARCH_NAME."
        return 0
    fi

    if [ -n "$VERSION" ]; then
        INSTALL_MODE="prebuilt"
        log_info "decision: installing the requested version $VERSION, latest.json is not consulted"
        return 0
    fi

    if resolve_latest_version; then
        INSTALL_MODE="prebuilt"
        return 0
    fi

    if [ -n "$PREBUILT_ONLY" ]; then
        err "could not resolve a published proa-cli release from $(display_url "$DOWNLOAD_BASE"); remove --prebuilt-only to allow a source build"
    fi

    INSTALL_MODE="source"
    log_info "decision: building from source because $DOWNLOAD_BASE/latest.json returned no usable version"
    MODE_NOTE="Could not find a published proa-cli release at $(display_url "$DOWNLOAD_BASE")."
    MODE_NOTE="$MODE_NOTE${NL}Either none has been published yet, or the download host is unreachable."
}

# ---------------------------------------------------------------------------
# Exit handling
# ---------------------------------------------------------------------------

# Runs on every exit path, including a successful one. err() has usually
# written the footer and printed the log path already, and both are guarded, so
# whichever gets here first wins and the other is a no-op.
on_exit() {
    local code="$1"

    cleanup

    if [ "$code" = "0" ]; then
        log_footer "success" "$code"
        print_log_hint_success
    else
        # Guarded, so a failure that went through err keeps the one headline it
        # already printed. This covers the paths that never reach err at all,
        # a Ctrl-C during a download most of all, which would otherwise end
        # with a log path and no statement that anything went wrong.
        fail_headline
        log_footer "failure" "$code"
        print_log_hint_failure
    fi
}

# $1 signal name, $2 exit code. Re-exits so the EXIT trap writes the footer.
on_signal() {
    log_write ERROR "interrupted by SIG$1"
    cleanup
    exit "$2"
}

# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

main() {
    set_defaults
    setup_colors
    setup_glyphs
    init_timer
    prescan_verbose "$@"
    init_log "$@"

    # These traps live in main's own body deliberately. zsh runs an EXIT trap
    # that was installed inside a function as soon as that function returns, so
    # registering them in a helper would delete the temp directory before the
    # download ever started. They are installed before any work starts so that
    # every exit path, including an argument error, still gets a log footer.
    # The signal traps cover a Ctrl-C during a slow download and re-exit so the
    # EXIT trap still fires.
    trap 'EXIT_STATUS=$?; on_exit "$EXIT_STATUS"' EXIT
    trap 'on_signal INT 130' INT
    trap 'on_signal TERM 143' TERM
    trap 'on_signal HUP 129' HUP

    parse_args "$@"
    log_inputs

    need_cmd grep "used to check your shell profile before editing it"
    need_cmd mkdir "used to create the install directory"

    # Everything from here to print_header is quiet on the console and writes
    # only to the log. It is what lets the headline block state the platform,
    # the toolchain, the source and the version as settled facts.
    log_section "platform"
    detect_platform
    detect_downloader
    make_tmp_dir
    check_log_outside_tmp
    resolve_bin_dir

    if [ -n "$TAILWIND_ONLY" ]; then
        log_info "decision: installing only Tailwind CSS CLI v$TAILWIND_VERSION"
        print_tailwind_only_header
        install_tailwind
        update_path
        print_tailwind_only_summary
        return 0
    fi

    probe_rust
    record_previous_version
    choose_install_mode

    print_header

    # After the block, because this is the half of the preflight that can warn,
    # prompt and install a toolchain.
    ensure_rust

    if [ "$INSTALL_MODE" = "source" ]; then
        install_from_source
        return 0
    fi

    install_prebuilt
}

main "$@"
