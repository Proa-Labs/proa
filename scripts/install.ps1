# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

<#
.SYNOPSIS
Installs the Proa CLI (the proa and cargo-proa binaries) on Windows.

.DESCRIPTION
Downloads the prebuilt proa-cli release archive that matches this machine,
verifies its sha256 against the published SHA256SUMS file, and installs both
binaries into %USERPROFILE%\.proa\bin. When no prebuilt archive matches, or no
release has been published yet, it builds from source with cargo instead.

The archives are fetched from https://proa.so/downloads/cli, a bridge in front
of the GitHub releases that hold the real files. Point -DownloadBase somewhere
else to install from a mirror, a fork or a local test server.

Every run also writes a log file, by default
%TEMP%\proa-install-<UTC timestamp>-<pid>.log, and prints its path at the end.
The console stays as quiet as it always was; the detail goes into the file so
there is something to attach to a bug report. Set PROA_LOG to choose the path,
or PROA_LOG=off to turn logging off.

This is the Windows counterpart of install.sh and behaves the same way.

Parameter binding only works when the script is on disk:

    irm https://proa.so/install.ps1 -OutFile install.ps1
    .\install.ps1 -Version 0.1.0

The documented one liner, "irm https://proa.so/install.ps1 | iex", evaluates the
script as a statement list. Nothing is being invoked, so there is nothing to
bind arguments to and -Version is ignored without any warning. Configure that
path with the PROA_* environment variables instead:

    $env:PROA_VERSION = '0.1.0'; irm https://proa.so/install.ps1 | iex

.PARAMETER Version
Version to install. Accepts 0.1.0, v0.1.0 or proa-cli-v0.1.0. Defaults to the
latest published release.

.PARAMETER To
Install the binaries into this directory instead of $PROA_HOME\bin.

.PARAMETER DownloadBase
Fetch latest.json and the release archives from this base URL instead of
https://proa.so/downloads/cli. A trailing slash is ignored.

.PARAMETER Repo
Build from this GitHub repository when a source build is needed. Downloads
never use it. Default: Proa-Labs/proa.

.PARAMETER NoModifyPath
Never change the user PATH. The installer still prints what to add.

.PARAMETER BuildFromSource
Skip the prebuilt download and run cargo install. With no version, build Git
HEAD; a version pins the release or snapshot source. Requires cargo and takes
several minutes.

.PARAMETER PrebuiltOnly
Fail if the matching release archive cannot be downloaded. Never fall back to
a source build. Use this in CI to keep installation deterministic and fast.

.PARAMETER Yes
Assume yes for every prompt. Use this in CI.

.PARAMETER VerboseLog
Also print the log file's DEBUG detail to the console. This is spelled
-VerboseLog rather than -Verbose on purpose; see the comment above
Set-Verbosity. On the "irm ... | iex" path, where no flag binds, set
$VerbosePreference = 'Continue' or PROA_VERBOSE=1 instead.

.PARAMETER Help
Print help and exit.

.EXAMPLE
irm https://proa.so/install.ps1 | iex

.EXAMPLE
$env:PROA_VERSION = '0.1.0'; irm https://proa.so/install.ps1 | iex

.EXAMPLE
$env:PROA_PREBUILT_ONLY = '1'; irm https://proa.so/install.ps1 | iex

.EXAMPLE
.\install.ps1 -Version 0.1.0 -To C:\tools\proa
#>

# This param block is valid under both entry points: as the first statement of a
# script file it binds real arguments, and inside the statement list that
# Invoke-Expression builds it simply initialises every variable to its default.
# That is why the PROA_* environment variables are the only way to configure the
# "irm ... | iex" one liner. Do not move this block and do not put executable
# statements above it, or PowerShell will reject the file.
#
# There is deliberately no [CmdletBinding()] here. Adding it would turn this
# file into an advanced script, which changes how the block behaves inside the
# Invoke-Expression statement list that the documented one liner relies on, and
# that path cannot be re-tested on every host this script runs on. It is also
# what would make -Verbose a common parameter, and a -Verbose switch of our own
# would then be a duplicate declaration. See Set-Verbosity.
param(
    [string]$Version = '',
    [string]$To = '',
    [string]$DownloadBase = '',
    [string]$Repo = '',
    [switch]$NoModifyPath,
    [switch]$BuildFromSource,
    [switch]$PrebuiltOnly,
    [switch]$Yes,
    [switch]$VerboseLog,
    [switch]$Help
)

# Set-StrictMode and $ErrorActionPreference live inside Invoke-Main, not here.
# See the comment there. Nothing above Invoke-Main needs either setting: this
# file is function definitions plus the constant assignments below.

# Windows PowerShell 5.1 still negotiates SSL 3.0 and TLS 1.0 by default, and
# proa.so and github.com both refuse them. Add TLS 1.2 before the first web
# request rather than replacing the list, so a host that already prefers
# TLS 1.3 keeps it.
try {
    [Net.ServicePointManager]::SecurityProtocol =
        [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch {
    # PowerShell 7 on some platforms leaves ServicePointManager inert. Its HTTP
    # stack already defaults to TLS 1.2 or better, so this is not fatal.
}

# Only the source build uses this: 'cargo install --git https://github.com/<repo>'.
# Every download goes through the base URL below instead, so the two settings
# are independent and the help text says so.
$script:DefaultRepo = 'Proa-Labs/proa'
# Where latest.json and the release archives are served from. proa.so puts a
# bridge in front of the GitHub releases that hold the real files, so the
# archives are reachable whether or not the repository is public.
$script:DefaultDownloadBase = 'https://proa.so/downloads/cli'
# Release tags are proa-cli-v<version>. Nothing builds a URL from this any more;
# it is here so -Version accepts a whole tag and strips it back to a version.
$script:TagPrefix = 'proa-cli-v'
$script:Msrv = '1.93.0'
# Sent on every request. Hosts in front of a release, github.com included,
# answer 403 to requests without a User-Agent, and it is what identifies an
# installer run in the download logs.
$script:UserAgent = 'proa-installer'
# Invoke-WebRequest defaults to an infinite timeout on Windows PowerShell 5.1, so
# a stalled connection hangs the installer forever with no output. Generous
# enough that a slow link still finishes a release archive.
$script:DownloadTimeoutSec = 300
# Thrown by Stop-Install when there is no script file to exit from. See the
# comment there for why "irm ... | iex" cannot use exit.
$script:AbortToken = 'proa-installer-abort'
# Version of any install already present in the target directory, so the version
# line can say what this run is replacing.
$script:PreviousVersion = ''
# Version the installed binary reports about itself, filled in by
# Confirm-Installation and printed in the closing headline.
$script:InstalledVersion = ''
# Set once a binary has actually been written. The failure block says "Nothing
# was installed" only while this is false, because saying it after a half
# finished copy would be a lie.
$script:FilesWritten = $false
# Warnings the Rust preflight wants to make, held back until the facts block has
# been printed so they do not split it in half.
$script:ToolchainNotes = @()
# What happened to PATH: 'changed', 'present', 'session' or 'manual'. The
# closing block tells the reader what to do next based on this.
$script:PathState = ''
# Evaluated with plain .NET so these lines depend on none of the functions
# below. Colour is for a person watching a terminal, so NO_COLOR turns it off
# and so does a redirected stdout: a pipe, a file or a CI transcript has to read
# as plain text.
$script:UseColor = [string]::IsNullOrEmpty([Environment]::GetEnvironmentVariable('NO_COLOR'))
if ($script:UseColor) {
    try {
        if ([Console]::IsOutputRedirected) { $script:UseColor = $false }
    } catch {
        # Some hosts have no console object at all. Leaving colour on costs
        # nothing there: Write-Host colours through the host, and a host without
        # a console ignores the request.
    }
}

# The step markers, in the ASCII spelling every terminal can render.
# Initialize-Console upgrades them to their UTF-8 glyphs once it has proved the
# console will not turn them into question marks, which is what a legacy code
# page does on Windows PowerShell 5.1. They are never written as literal
# characters in this file: a .ps1 without a BOM is read as the ANSI code page by
# 5.1, and "irm ... | iex" decodes the response with whatever charset the
# response headers imply, so a literal U+2713 in the source is mojibake waiting
# to happen. Building them from code points sidesteps both.
$script:GlyphStep = '->'
$script:GlyphOk = 'ok'
$script:GlyphFail = 'x'
# Width of the marker column, which is the width of the widest marker in the set
# in use: two for the ASCII stand-ins, one once the glyphs replace them. The step
# lines pad to it so the label column starts in the same place on every line.
$script:GlyphWidth = 2
$script:UseUnicode = $false
# Repository the run ended up using, so the "report this" line can name the
# right issue tracker even when the failure happened before Invoke-Main got
# that far.
$script:Repository = $script:DefaultRepo
# Set by Get-MachineArchitecture so the log can say which of the three possible
# sources actually answered. Kept as a side effect rather than a second
# function so the precedence lives in exactly one place.
$script:ArchitectureSource = ''

# Logging state. All of it is script scoped, and Close-Log clears the writer,
# because under "irm ... | iex" these variables live in the caller's session
# and an open file handle must not outlive the run.
$script:LogPath = ''
$script:LogWriter = $null
$script:LogEnabled = $false
$script:LogWarned = $false
$script:LogStart = [DateTime]::UtcNow
$script:FooterWritten = $false
$script:Verbose = $false
$script:VerboseSource = ''
# '' until probed, then 'yes' or 'no'. See Test-DownloadPassThru.
$script:PassThruProbe = ''

# Captured at file scope on purpose: inside a function $MyInvocation describes
# that function's call, not this script's.
$script:InvocationLine = ''
try {
    if ($null -ne $MyInvocation -and $null -ne $MyInvocation.Line) {
        $script:InvocationLine = ([string]$MyInvocation.Line).Trim()
    }
} catch {
    # Some hosts leave $MyInvocation without a Line. The log simply omits it.
}
$script:ProcessCommandLine = ''
try {
    $script:ProcessCommandLine = [Environment]::CommandLine
} catch {
    # Not available under every host or trust level, and not worth failing over.
}

# ---------------------------------------------------------------------------
# Logging
#
# This installer runs on machines nobody will ever see, usually through
# "irm ... | iex", and when it fails the console scrollback is the only
# evidence there is, if the window is even still open. So every console message
# is mirrored into a file, together with the detail the console deliberately
# keeps to itself: raw platform values, every HTTP request and its status, both
# checksums, the archive contents, and every fallback decision with its reason.
# Optimise for "someone pastes this file into an issue and the failure is
# obvious", not for a prettier terminal.
#
# Start-Transcript is deliberately not used. It records the whole host session
# rather than this script, it behaves badly when the script is a statement list
# inside Invoke-Expression, and its free-form output cannot be structured. The
# records below are written explicitly, one line each, as
# "<UTC ISO-8601> <LEVEL> <message>" with levels INFO / WARN / ERROR / DEBUG,
# which is the same shape install.sh writes so the two logs read alike.
#
# Failing to log is never fatal. A read-only temp directory, a full disk or a
# PROA_LOG the user cannot write all degrade to console-only output with one
# warning.
# ---------------------------------------------------------------------------

function Get-UtcText {
    param([string]$Format)

    # DateTime.ToString formats through the current culture unless it is told
    # not to, and ':' in a custom format string means "the culture's time
    # separator" rather than a literal colon. A Thai locale would also print a
    # Buddhist year, so a log written there would not even sort next to one
    # written here. InvariantCulture pins all of it, and both this API and the
    # format string work on Windows PowerShell 5.1.
    return [DateTime]::UtcNow.ToString($Format, [System.Globalization.CultureInfo]::InvariantCulture)
}

function Get-ElapsedText {
    $elapsed = ([DateTime]::UtcNow - $script:LogStart).TotalSeconds
    # Same reason as the timestamps: a German locale would otherwise write 12,4
    # and a reader would wonder what happened.
    return $elapsed.ToString('0.0', [System.Globalization.CultureInfo]::InvariantCulture)
}

# Names that must never carry their value into a file people are encouraged to
# paste into a public issue. CARGO_REGISTRY_TOKEN is a real variable
# users export, and it is easy to write it on the same line as the install
# command. Nothing here ever enumerates the environment; only the specific
# PROA_* and NO_COLOR names are read by name and logged. This pattern is the
# second line of defence, for text that arrives from somewhere else entirely,
# such as the process command line.
$script:SensitivePattern = '(?i)([A-Za-z0-9_]*(TOKEN|SECRET|KEY|PASSWORD|PASSWD|CREDENTIAL|AUTH)[A-Za-z0-9_]*)\s*=\s*(''[^'']*''|"[^"]*"|\S+)'

# A credential can also arrive inside a URL, as the userinfo of a cargo
# registry index or of a private download mirror:
#
#     $env:CARGO_REGISTRIES_EXAMPLE_INDEX = 'sparse+https://bot:tok@registry.example/sparse/'
#
# The name-based pattern above cannot catch that one, because the variable
# carrying the secret is CARGO_REGISTRIES_EXAMPLE_INDEX and its name holds none of
# the words listed there. It matters here and not in install.sh because the log
# header records [Environment]::CommandLine, and the documented one liner is
# exactly the shape that puts an assignment on the same line as the install.
#
# Only the authority is searched, so a path that legitimately contains '@' is
# left intact, and the host and path survive because that is the part worth
# having in a bug report. This is the counterpart of redact_url_userinfo in
# install.sh.
$script:UrlUserInfoPattern = '(?i)\b([A-Za-z][A-Za-z0-9+.-]*://)[^/\s@]+@'

function Protect-Sensitive {
    param([string]$Text)

    if ([string]::IsNullOrEmpty($Text)) { return '' }
    try {
        $masked = [regex]::Replace($Text, $script:SensitivePattern, '$1=<redacted>')
        return [regex]::Replace($masked, $script:UrlUserInfoPattern, '$1<redacted>@')
    } catch {
        return $Text
    }
}

function Write-LogLine {
    param([string]$Level, [string]$Message)

    if (-not $script:LogEnabled) { return }
    if ($null -eq $script:LogWriter) { return }
    # Blank lines are console spacing rather than content, so they are not
    # mirrored. Nothing structured passes an empty message.
    if ([string]::IsNullOrEmpty($Message) -or [string]::IsNullOrEmpty($Message.Trim())) { return }

    $stamp = Get-UtcText 'yyyy-MM-ddTHH:mm:ssZ'
    # One record per line. A message that spans lines, such as a captured tool
    # output, becomes several records rather than one ragged one, so the file
    # stays greppable by level.
    $lines = (Protect-Sensitive $Message) -split "`r`n|`n|`r"
    try {
        foreach ($line in $lines) {
            $script:LogWriter.WriteLine($stamp + ' ' + $Level + ' ' + $line)
        }
    } catch {
        Disable-Log ('writing to ' + $script:LogPath + ' failed: ' + $_.Exception.Message)
    }
}

# The one piece of shape this file has. Records are never aligned, never
# reordered and never reformatted, because the log's value is that it is boring
# and greppable, but a blank line and a "--- name ---" marker between the phases
# make a long file skimmable and cost nothing to any tool reading it. The
# markers are the same five install.sh writes, so the two logs still read alike.
function Write-LogSection {
    param([string]$Name)

    if (-not $script:LogEnabled) { return }
    if ($null -eq $script:LogWriter) { return }
    # Written straight to the writer: Write-LogLine drops empty messages, which
    # is right for content and wrong for a deliberate separator.
    try {
        $script:LogWriter.WriteLine('')
    } catch {
        Disable-Log ('writing to ' + $script:LogPath + ' failed: ' + $_.Exception.Message)
        return
    }
    Write-LogLine 'INFO' ('--- ' + $Name + ' ---')
}

function Disable-Log {
    param([string]$Reason)

    $script:LogEnabled = $false
    if ($script:LogWarned) { return }
    $script:LogWarned = $true
    Write-Warn ('continuing without a log file. ' + $Reason)
}

# Two spaces of indent for the grouped key/value records, so a settings block or
# a platform block reads as one thing in the file.
function Write-Setting {
    param([string]$Name, [string]$Value, [string]$Source)

    if ([string]::IsNullOrEmpty($Value)) {
        Write-LogLine 'INFO' ('  ' + $Name + ' = (unset)')
        return
    }
    if ([string]::IsNullOrEmpty($Source)) {
        Write-LogLine 'INFO' ('  ' + $Name + ' = ' + $Value)
        return
    }
    Write-LogLine 'INFO' ('  ' + $Name + ' = ' + $Value + ' (' + $Source + ')')
}

function Write-SettingFlag {
    param([string]$Name, [bool]$Value, [string]$Source)

    if ($Value) {
        Write-Setting $Name 'yes' $Source
    } else {
        Write-Setting $Name 'no' 'default'
    }
}

function Write-Fact {
    param([string]$Name, [string]$Value)

    if ([string]::IsNullOrEmpty($Value)) {
        Write-LogLine 'INFO' ('  ' + $Name + ': (unset)')
        return
    }
    Write-LogLine 'INFO' ('  ' + $Name + ': ' + $Value)
}

function Get-LogDirectory {
    # The log must live outside the temp working directory this script creates,
    # because that directory is removed when the run ends. A log deleted on
    # failure is worse than no log at all, since the run then looks like it
    # worked. New-TempDirectory makes a proa-install-<guid> directory under the
    # same temp root, so the log file is a sibling of it and never inside it.
    # Remove-TempDirectory checks that again at delete time, because PROA_LOG
    # can name any path at all.
    if (Test-IsWindows) {
        $temp = Get-EnvValue 'TEMP'
        if ([string]::IsNullOrEmpty($temp)) { $temp = Get-EnvValue 'TMP' }
        if ([string]::IsNullOrEmpty($temp)) { $temp = [System.IO.Path]::GetTempPath() }
        return $temp
    }
    # PowerShell 7 on Linux or macOS only reaches this script to be told to run
    # install.sh instead, and that message is worth writing where install.sh
    # would have written it.
    return '/tmp'
}

function Get-ScriptIdentity {
    if (Test-RunningAsFile) {
        return [string](Get-Variable -Name 'PSCommandPath' -ValueOnly)
    }
    return 'install.ps1 (evaluated by Invoke-Expression, not on disk)'
}

function Get-HostDescription {
    $version = ''
    $edition = ''
    try {
        # -ErrorAction SilentlyContinue as well as the catch: a missing variable
        # is a non-terminating error here, which the catch would not see and
        # which would print a raw PowerShell error to a console that has not
        # said anything yet. This runs before Invoke-Main sets $ErrorActionPreference.
        $table = Get-Variable -Name 'PSVersionTable' -ValueOnly -ErrorAction SilentlyContinue
        if ($null -ne $table) {
            if ($table.ContainsKey('PSVersion')) { $version = [string]$table['PSVersion'] }
            if ($table.ContainsKey('PSEdition')) { $edition = [string]$table['PSEdition'] }
        }
    } catch {
        # $PSVersionTable predates every host this runs on, but reading it must
        # not be the thing that breaks the installer.
    }
    if ([string]::IsNullOrEmpty($version)) { return 'PowerShell (version unavailable)' }
    if ([string]::IsNullOrEmpty($edition)) { return ('PowerShell ' + $version) }
    return ('PowerShell ' + $version + ' (' + $edition + ')')
}

function Write-LogHeader {
    Write-LogLine 'INFO' '=== proa installer ==='
    Write-LogLine 'INFO' ('script: ' + (Get-ScriptIdentity))
    # Nothing stamps a release into this file, so there is no revision to
    # report. The version that ends up installed is logged further down.
    Write-LogLine 'INFO' 'installer revision: not embedded in this script'
    if (-not [string]::IsNullOrEmpty($script:InvocationLine)) {
        Write-LogLine 'INFO' ('invocation: ' + $script:InvocationLine)
    }
    if (-not [string]::IsNullOrEmpty($script:ProcessCommandLine)) {
        Write-LogLine 'INFO' ('process command line: ' + $script:ProcessCommandLine)
    }
    Write-LogLine 'INFO' ('started: ' + (Get-UtcText 'yyyy-MM-ddTHH:mm:ssZ'))
    Write-LogLine 'INFO' ('log file: ' + $script:LogPath)
    Write-LogLine 'INFO' ('host: ' + (Get-HostDescription))
    # What the console decided about itself. A report that says "the check marks
    # came out as question marks" is answered by this line.
    Write-LogLine 'INFO' ('console: ' + (Get-ConsoleDescription))

    $verboseText = 'off'
    if ($script:Verbose) { $verboseText = 'on' }
    if (-not [string]::IsNullOrEmpty($script:VerboseSource)) {
        $verboseText = $verboseText + ' (' + $script:VerboseSource + ')'
    }
    Write-LogLine 'INFO' ('verbose console output: ' + $verboseText)
}

# Windows PowerShell 5.1 never syncs the process working directory with the
# shell's own location, and the two halves of the code below disagree about
# which one a relative PROA_LOG means: New-Item and Test-Path go through the
# provider and use Get-Location, while StreamWriter and [IO.Path]::GetFullPath
# are plain .NET and use the directory powershell.exe started in. Left alone,
# "PROA_LOG=install.log" creates the parent in one place, writes the file in
# another or fails to open it at all, prints a relative path the reader cannot
# resolve, and gives Test-PathUnder a path that may appear to sit inside the
# temp directory the cleanup deletes. Anchoring the value once, here, is what
# makes the rest of the file agree: everything downstream sees an absolute path.
function Resolve-LogPath {
    param([string]$Path)

    if ([string]::IsNullOrEmpty($Path)) { return '' }
    try {
        if ([System.IO.Path]::IsPathRooted($Path)) { return [System.IO.Path]::GetFullPath($Path) }
        # -PSProvider FileSystem so a session sitting on HKLM: or Cert: still
        # yields a directory, and ProviderPath so a PSDrive is expanded to the
        # real path .NET needs rather than the drive-qualified one.
        $base = [string](Get-Location -PSProvider FileSystem).ProviderPath
        if ([string]::IsNullOrEmpty($base)) { return [System.IO.Path]::GetFullPath($Path) }
        return [System.IO.Path]::GetFullPath((Join-Path $base $Path))
    } catch {
        # A path .NET refuses to normalise is reported as it was given, and the
        # open below turns it into the one warning this feature is allowed.
        return $Path
    }
}

function Initialize-Log {
    $script:LogStart = [DateTime]::UtcNow

    $requested = Get-EnvValue 'PROA_LOG'
    if ($requested.Trim().ToLowerInvariant() -eq 'off') {
        # Explicitly turned off. Nothing is written and nothing is said about
        # it, because silence is what was asked for.
        return
    }

    $path = $requested.Trim()
    if ([string]::IsNullOrEmpty($path)) {
        $name = 'proa-install-' + (Get-UtcText 'yyyyMMddTHHmmssZ') + '-' + $PID + '.log'
        $path = Join-Path (Get-LogDirectory) $name
    }
    $path = Resolve-LogPath $path

    try {
        $parent = Split-Path -Path $path -Parent
        if (-not [string]::IsNullOrEmpty($parent) -and -not (Test-Path -LiteralPath $parent)) {
            # -ErrorAction Stop because this runs before Invoke-Main sets
            # $ErrorActionPreference: without it a PROA_LOG under a directory
            # nobody can create would print a raw PowerShell error of its own
            # before the one warning below.
            New-Item -ItemType Directory -Path $parent -Force -ErrorAction Stop | Out-Null
        }

        # Out-File, Add-Content and > all default to UTF-16 on Windows
        # PowerShell 5.1, which turns a log somebody pastes into an issue into
        # mojibake and doubles its size on the way. A StreamWriter with an
        # explicit BOM-less UTF-8 encoding is the one spelling that behaves the
        # same on 5.1 and on PowerShell 7.
        #
        # Append rather than truncate, so a PROA_LOG pointing at one fixed file
        # keeps the history instead of losing the run before it. AutoFlush, so
        # a run killed mid-download still leaves everything up to that point on
        # disk, which is exactly the run whose log matters most.
        $encoding = New-Object -TypeName 'System.Text.UTF8Encoding' -ArgumentList $false
        $writer = New-Object -TypeName 'System.IO.StreamWriter' -ArgumentList @($path, $true, $encoding)
        $writer.AutoFlush = $true
        $script:LogWriter = $writer
        $script:LogPath = $path
        $script:LogEnabled = $true
    } catch {
        Disable-Log ('could not open ' + $path + ': ' + $_.Exception.Message)
        return
    }

    Write-LogHeader
}

function Complete-Log {
    param([string]$Outcome, [int]$ExitCode)

    # Whichever path gets here first owns the footer. Stop-Install writes it
    # before unwinding, and the entry point writes it on the way out, so this
    # runs twice on a failure that starts inside a function.
    if ($script:FooterWritten) { return }
    $script:FooterWritten = $true
    Write-LogSection 'summary'
    Write-LogLine 'INFO' ('outcome: ' + $Outcome + ', exit code ' + $ExitCode + ', elapsed ' + (Get-ElapsedText) + 's')
    Write-LogLine 'INFO' '=== end of proa installer log ==='
}

function Close-Log {
    if ($null -eq $script:LogWriter) { return }
    try {
        $script:LogWriter.Flush()
        $script:LogWriter.Dispose()
    } catch {
        # Nothing useful is left to do about a log file at this point.
    }
    # Cleared, not just closed. Under "irm ... | iex" these variables live in
    # the caller's session, and a stale writer there would keep the file locked
    # for the rest of that session.
    $script:LogWriter = $null
    $script:LogEnabled = $false
}

function Show-LogPath {
    param([bool]$Failed)

    # Keyed on the path rather than on $script:LogEnabled, so a log that was
    # opened and then abandoned mid-run (a full disk, say) is still named: the
    # part that was written is exactly the part worth reading.
    if ([string]::IsNullOrEmpty($script:LogPath)) { return }

    # Console only, on purpose, which is why these are Write-Colored rather than
    # the mirroring helpers of the same shape. The path is already in the log's
    # own header, and the footer has closed the record by the time this runs, so
    # anything written here would land after the end marker.
    #
    # Both forms use the same "Log" label as the closing block, so the line looks
    # the same wherever a reader meets it. On the failure path it is followed by
    # the one request that makes the file useful to anybody else.
    if ($Failed) {
        Write-Blank
        Write-Colored (Format-Closing 'Log' $script:LogPath) 'DarkGray'
        Write-Colored '  Please attach the log if you report this:' 'DarkGray'
        Write-Colored ('    https://github.com/' + $script:Repository + '/issues') ''
        return
    }
    Write-Colored (Format-Closing 'Log' $script:LogPath) 'DarkGray'
}

# -Verbose is a PowerShell common parameter, but only when [CmdletBinding()] is
# in play. This script deliberately has no [CmdletBinding()] (see the param
# block), so there is no common -Verbose to use and declaring a -Verbose switch
# of our own would be a trap: it would work today and become a duplicate
# declaration the moment anyone added the attribute. The switch is therefore
# named -VerboseLog, which cannot collide with anything.
#
# The built-in is still honoured where it exists as state rather than as a
# parameter: $VerbosePreference set to Continue turns this on. That is also the
# only way to get verbose output from "irm ... | iex", where no flag binds at
# all:
#
#     $VerbosePreference = 'Continue'; irm https://proa.so/install.ps1 | iex
#     $env:PROA_VERBOSE = '1'; irm https://proa.so/install.ps1 | iex
function Set-Verbosity {
    param([bool]$Requested)

    if ($Requested) {
        $script:Verbose = $true
        $script:VerboseSource = 'flag -VerboseLog'
        return
    }
    if (Test-EnvFlag 'PROA_VERBOSE') {
        $script:Verbose = $true
        $script:VerboseSource = 'PROA_VERBOSE'
        return
    }
    if (Test-Path -Path 'Variable:\VerbosePreference') {
        $preference = [string](Get-Variable -Name 'VerbosePreference' -ValueOnly)
        if ($preference -eq 'Continue' -or $preference -eq 'Inquire') {
            $script:Verbose = $true
            $script:VerboseSource = '$VerbosePreference'
            return
        }
    }
    $script:Verbose = $false
    $script:VerboseSource = ''
}

# ---------------------------------------------------------------------------
# Output
#
# The console prints one of five shapes and nothing else, so a run reads as a
# short table rather than as a paragraph. install.sh prints the same five, which
# is what makes the two installers look alike side by side:
#
#     Proa CLI installer                       headline    column 0
#
#       platform   Windows AMD64 (...)         fact        2 + label 11 + value
#       version    0.1.0
#
#       -> downloading   proa-cli-...zip       step        2 + marker + 1
#       ok checksum      sha256 verified                   + label 14 + value
#
#     proa 0.1.0 is ready.                     headline    column 0
#
#       Then create a site:                    note        2 + text
#         proa new site hello-proa ...         command     4 + text
#
#       Docs  https://proa.so/docs             closing     2 + label 6 + value
#
# Every width is a format specifier, '{0,-11}' and friends, never hand counted
# spaces. The alignment is the whole point of the layout, and a label that grows
# by a character must not be able to break the column quietly.
#
# The marker field is as wide as the widest marker in the set actually in use,
# two for the ASCII stand-ins and one for the UTF-8 glyphs, so 'ok' and 'x' line
# up with each other without the glyphs paying for a column they do not need.
#
# Colour is deliberately thin: emphasis on the two headlines, dim for the label
# columns and the closing lines, green on a check mark, red on a cross. Write-Host
# has no bold attribute, so White stands in for it, and DarkGray for dim. The
# colours are host attributes rather than ANSI escapes, so nothing leaks into a
# redirected stream, and $script:UseColor turns them off anyway.
#
# Each of these also mirrors its message into the log, without the decoration,
# because colour and indentation in a pasted file are just noise:
#
#     Write-Headline  -> INFO
#     Write-KeyValue  -> INFO, as "name: value"
#     Write-Step*     -> INFO, or WARN for a failed step
#     Write-Note      -> INFO
#     Write-Command   -> INFO
#     Write-Closing   -> INFO
#     Write-Warn      -> WARN   (the "warning" marker is dropped, the level says it)
#     Write-Problem   -> ERROR
#
# Write-Diagnostic is the one that is not a console message at all: it records
# the detail that only belongs in the file, and reaches the console only in
# verbose mode.
# ---------------------------------------------------------------------------

# True when this process still has a console to draw on. A redirected stdout
# means a file, a pipe or a CI transcript is reading, and neither colour nor a
# progress bar belongs there.
function Test-ConsoleOutput {
    try {
        return (-not [Console]::IsOutputRedirected)
    } catch {
        # Hosts without a console object at all. Nothing here is worth failing
        # over, and the caller's other tests still apply.
        return $true
    }
}

# Whether the UTF-8 markers can be printed without turning into question marks.
# Two independent questions, and both have to answer yes.
function Test-UnicodeConsole {
    # First, what the environment asked for. LC_ALL wins over LC_CTYPE, which
    # wins over LANG, the usual POSIX precedence, and only the first one that is
    # set is consulted: a session that deliberately asked for C, or for a legacy
    # code page, gets the ASCII markers even on a console that could render
    # more. These are usually unset on Windows, which is silence rather than a
    # veto, so the encoding probe below decides on its own there.
    $locale = ''
    foreach ($name in @('LC_ALL', 'LC_CTYPE', 'LANG')) {
        $value = [Environment]::GetEnvironmentVariable($name)
        if (-not [string]::IsNullOrEmpty($value)) {
            $locale = $value
            break
        }
    }
    if (-not [string]::IsNullOrEmpty($locale)) {
        $lower = $locale.ToLowerInvariant()
        if (-not ($lower.Contains('utf-8') -or $lower.Contains('utf8'))) { return $false }
    }

    # Second, whether the console can carry the characters at all. This is the
    # half that matters on Windows PowerShell 5.1, where [Console]::OutputEncoding
    # is very often code page 437 or 1252. Those encoders map U+2713 to '?', so
    # the check mark would arrive as a question mark and the run would look
    # broken for no reason. Round tripping the three glyphs through the encoding
    # answers the question for any code page without hard coding a list of them.
    try {
        $encoding = [Console]::OutputEncoding
        if ($null -eq $encoding) { return $false }
        $sample = ([string][char]0x25B8) + ([string][char]0x2713) + ([string][char]0x2717)
        if ($encoding.GetString($encoding.GetBytes($sample)) -ne $sample) { return $false }
        return $true
    } catch {
        # No console attached, or a host that refuses the property. Mojibake on
        # a CI runner is worse than a plain ASCII arrow, so when in doubt this
        # falls back.
        return $false
    }
}

# Runs before anything is printed, including the log header, which records what
# it decided.
function Initialize-Console {
    if (Test-UnicodeConsole) {
        $script:UseUnicode = $true
        # U+25B8 BLACK RIGHT-POINTING SMALL TRIANGLE, U+2713 CHECK MARK,
        # U+2717 BALLOT X. Built from code points rather than written as literal
        # characters; see the comment on the ASCII defaults for why.
        $script:GlyphStep = [string][char]0x25B8
        $script:GlyphOk = [string][char]0x2713
        $script:GlyphFail = [string][char]0x2717
        $script:GlyphWidth = 1
    }
}

function Get-ConsoleDescription {
    $colourText = 'colour off'
    if ($script:UseColor) { $colourText = 'colour on' }
    $glyphText = 'ascii markers'
    if ($script:UseUnicode) { $glyphText = 'utf-8 markers' }
    return ($colourText + ', ' + $glyphText)
}

function Write-Colored {
    param([string]$Text, [string]$Color)

    if ($script:UseColor -and -not [string]::IsNullOrEmpty($Color)) {
        Write-Host $Text -ForegroundColor $Color
    } else {
        Write-Host $Text
    }
}

# Console spacing, never mirrored: a blank line is layout, not content.
function Write-Blank {
    Write-Host ''
}

function Write-Headline {
    param([string]$Text, [string]$Color)

    $shade = $Color
    if ([string]::IsNullOrEmpty($shade)) { $shade = 'White' }
    Write-Colored $Text $shade
    Write-LogLine 'INFO' $Text
}

# The platform / toolchain / source / version block.
function Write-KeyValue {
    param([string]$Name, [string]$Value)

    # Padded only when something follows it, so an empty value cannot leave
    # invisible trailing whitespace on the line.
    $label = $Name
    if (-not [string]::IsNullOrEmpty($Value)) { $label = '{0,-11}' -f $Name }
    if ($script:UseColor) {
        Write-Host '  ' -NoNewline
        Write-Host $label -ForegroundColor 'DarkGray' -NoNewline
        Write-Host $Value
    } else {
        Write-Host ('  ' + $label + $Value)
    }
    Write-LogLine 'INFO' ($Name + ': ' + $Value)
}

function Write-StepLine {
    param([string]$Marker, [string]$Color, [string]$Label, [string]$Value)

    # The width comes from the marker set in use, so 'ok' and 'x' line up with
    # each other in ASCII mode without the UTF-8 mode paying for a column it does
    # not need. Still the format operator, just with the width supplied.
    $marker = ('{0,-' + $script:GlyphWidth + '}') -f $Marker
    # An empty value would otherwise leave the padding as trailing whitespace on
    # the line, which is invisible until someone diffs the output.
    $label = $Label
    if (-not [string]::IsNullOrEmpty($Value)) { $label = '{0,-14}' -f $Label }

    if ($script:UseColor) {
        Write-Host '  ' -NoNewline
        Write-Host $marker -ForegroundColor $Color -NoNewline
        Write-Host ' ' -NoNewline
        Write-Host $label -ForegroundColor 'DarkGray' -NoNewline
        Write-Host $Value
    } else {
        Write-Host ('  ' + $marker + ' ' + $label + $Value)
    }
}

# A step that happened, with no verdict of its own. The line that follows it
# usually carries the verdict.
function Write-StepStart {
    param([string]$Label, [string]$Value)

    Write-StepLine $script:GlyphStep 'DarkGray' $Label $Value
    Write-LogLine 'INFO' ($Label + ': ' + $Value)
}

function Write-StepOk {
    param([string]$Label, [string]$Value)

    Write-StepLine $script:GlyphOk 'Green' $Label $Value
    Write-LogLine 'INFO' ($Label + ': ' + $Value)
}

function Write-StepFail {
    param([string]$Label, [string]$Value)

    Write-StepLine $script:GlyphFail 'Red' $Label $Value
    Write-LogLine 'WARN' ($Label + ': ' + $Value)
}

# Secondary prose: an explanation, or the sentence introducing a command.
function Write-Note {
    param([string]$Message)

    Write-Colored ('  ' + $Message) 'DarkGray'
    Write-LogLine 'INFO' $Message
}

# Something to copy. Left undimmed on purpose: this is the one kind of line a
# reader is expected to select and paste.
function Write-Command {
    param([string]$Message)

    Write-Colored ('    ' + $Message) ''
    Write-LogLine 'INFO' $Message
}

# The shape of the Docs and Log lines that close a run. Separate from the
# printing so Show-LogPath can reuse the layout without the log mirroring, which
# it must not do: it runs after the footer has closed the record.
function Format-Closing {
    param([string]$Name, [string]$Value)

    return ('  ' + ('{0,-6}' -f $Name) + $Value)
}

function Write-Closing {
    param([string]$Name, [string]$Value)

    Write-Colored (Format-Closing $Name $Value) 'DarkGray'
    Write-LogLine 'INFO' ($Name + ': ' + $Value)
}

# Both of these use the same label width as Write-KeyValue, so a debug or
# warning line starts its text in the same column as a fact value and nothing
# looks ragged next to the block it interrupts. The width has to match that
# function exactly, not the length of the longest label here: these widths
# include the gap that separates a label from its value, so a narrower one
# silently pulls the text two columns left of every fact in the block above.
function Write-Diagnostic {
    param([string]$Message)

    Write-LogLine 'DEBUG' $Message
    # Nothing is echoed once the footer is written: on a failure the log path
    # has to be the last thing on the screen, and cleanup runs while the error
    # is still unwinding.
    if ($script:Verbose -and -not $script:FooterWritten) {
        Write-Colored ('  ' + ('{0,-11}' -f 'debug') + $Message) 'DarkGray'
    }
}

function Write-Warn {
    param([string]$Message)

    # Yellow on the label alone rather than on the whole line. A warning has to
    # be findable in a scrollback, but it is not the headline of the run and
    # should not read like one.
    $label = '{0,-11}' -f 'warning'
    if ($script:UseColor) {
        Write-Host '  ' -NoNewline
        Write-Host $label -ForegroundColor 'Yellow' -NoNewline
        Write-Host $Message
    } else {
        Write-Host ('  ' + $label + $Message)
    }
    Write-LogLine 'WARN' $Message
}

function Write-Problem {
    param([string]$Message)

    Write-Colored ('  ' + $Message) ''
    Write-LogLine 'ERROR' $Message
}

# The last thing a failed run prints before the log path. Kept obvious and kept
# useful: what went wrong, whatever detail helps a person compare values by eye,
# and whether anything was left behind.
#
# $Details lines are printed at the same indent as the message, so a caller that
# wants a command or a hash under it indents its own string by two more spaces.
# An empty string is a blank separator line.
function Show-Failure {
    param([string]$Message, [string[]]$Details)

    Write-Blank
    Write-Headline 'Installation failed.' 'Red'
    Write-Blank
    Write-Problem $Message

    if ($null -ne $Details) {
        foreach ($line in $Details) {
            if ([string]::IsNullOrEmpty($line)) {
                Write-Blank
                continue
            }
            Write-Colored ('  ' + $line) ''
            Write-LogLine 'ERROR' $line
        }
    }

    # Only while it is true. After a partial copy it would be a lie, and the
    # reader needs to know the difference.
    if (-not $script:FilesWritten) {
        Write-Blank
        Write-Colored '  Nothing was installed.' ''
        Write-LogLine 'INFO' 'nothing was installed'
    }
}

function Test-RunningAsFile {
    # $PSCommandPath holds the script's own path when this file was run from
    # disk, and is empty inside the statement list Invoke-Expression builds.
    # Reading it through the Variable: provider keeps Set-StrictMode happy on
    # hosts where the automatic variable is absent.
    if (Test-Path -Path 'Variable:\PSCommandPath') {
        $value = Get-Variable -Name 'PSCommandPath' -ValueOnly
        return (-not [string]::IsNullOrEmpty($value))
    }
    return $false
}

# Abort after the caller has already printed its own explanation.
function Stop-InstallSilently {
    # Finish the record before unwinding. The console is about to be the user's
    # only other evidence and it may well scroll away, so the file gets its
    # footer and the screen gets the path to it, in that order.
    Complete-Log 'failure' 1
    Show-LogPath $true
    # Closed here rather than only in the entry point's finally: on the exit
    # path below an unclosed writer would leave the log locked inside the
    # caller's interactive session.
    Close-Log

    # Under "irm ... | iex" there is no script to exit from, so exit terminates
    # the caller's PowerShell session: the window closes and the error scrolls
    # away before anyone can read it. Throwing unwinds the statement list
    # instead and leaves the console alive, and the entry point swallows the
    # token so nothing is printed twice.
    #
    # exit is still right everywhere else. From a script file it only leaves the
    # script, and in a non-interactive host (CI running
    # 'powershell -Command "irm ... | iex"') it is the only way to report a
    # non-zero exit code, because an error caught inside Invoke-Expression does
    # not reach the process exit code.
    if ((Test-RunningAsFile) -or -not (Test-Interactive)) { exit 1 }
    throw $script:AbortToken
}

function Stop-Install {
    param([string]$Message, [string[]]$Details)

    Show-Failure $Message $Details
    Stop-InstallSilently
}

# ---------------------------------------------------------------------------
# Small helpers
# ---------------------------------------------------------------------------

function Get-EnvValue {
    param([string]$Name)

    # The .NET accessor returns $null for names that were never set, which keeps
    # Set-StrictMode out of the picture entirely.
    $value = [Environment]::GetEnvironmentVariable($Name)
    if ($null -eq $value) { return '' }
    return $value
}

function Test-EnvFlag {
    param([string]$Name)

    # Any non-empty value counts as set, the same rule install.sh uses.
    return (-not [string]::IsNullOrEmpty((Get-EnvValue $Name)))
}

function Get-UserHome {
    $profilePath = Get-EnvValue 'USERPROFILE'
    if (-not [string]::IsNullOrEmpty($profilePath)) { return $profilePath }
    return [Environment]::GetFolderPath('UserProfile')
}

function Get-CargoBinDir {
    $cargoHome = Get-EnvValue 'CARGO_HOME'
    if ([string]::IsNullOrEmpty($cargoHome)) {
        $cargoHome = Join-Path (Get-UserHome) '.cargo'
    }
    return (Join-Path $cargoHome 'bin')
}

function Test-IsWindows {
    # $IsWindows exists only on PowerShell 6 and newer, and referencing it
    # directly would trip Set-StrictMode on Windows PowerShell 5.1. That edition
    # is Windows only by construction, so a missing variable means Windows.
    if (Test-Path -Path 'Variable:\IsWindows') {
        return [bool](Get-Variable -Name 'IsWindows' -ValueOnly)
    }
    return $true
}

function Test-Interactive {
    # A prompt is only honest when someone can answer it. UserInteractive is
    # false for services and some CI agents, and redirected input means the
    # caller piped something in that Read-Host would eat.
    if (-not [Environment]::UserInteractive) { return $false }
    try {
        if ([Console]::IsInputRedirected) { return $false }
    } catch {
        # IsInputRedirected is missing on very old hosts. Assume interactive.
    }
    return $true
}

function Get-LastExitCode {
    # $LASTEXITCODE does not exist until the session has run a native command,
    # and Set-StrictMode turns reading it before that into an error.
    if (Test-Path -Path 'Variable:\LASTEXITCODE') {
        $value = Get-Variable -Name 'LASTEXITCODE' -ValueOnly
        if ($null -ne $value) { return [int]$value }
    }
    return 0
}

function Get-CommandPath {
    param([string]$Name)

    $command = Get-Command -Name $Name -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $command) { return '' }
    if ($command.PSObject.Properties.Name -contains 'Source') { return [string]$command.Source }
    return ''
}

# For the log, where an exact byte count is worth more than a tidy one.
function Get-FileSizeText {
    param([string]$Path)

    try {
        if (-not (Test-Path -LiteralPath $Path)) { return 'no file written' }
        return ([string](Get-Item -LiteralPath $Path).Length + ' bytes')
    } catch {
        return 'size unavailable'
    }
}

# -1 when the size cannot be read, which Format-ByteSize renders as an empty
# string, so a console line simply omits the size rather than inventing one.
function Get-FileSizeBytes {
    param([string]$Path)

    try {
        if (-not (Test-Path -LiteralPath $Path)) { return [long](-1) }
        return [long](Get-Item -LiteralPath $Path).Length
    } catch {
        return [long](-1)
    }
}

# "10.4 MB" for the console. install.sh computes this with integer arithmetic
# because POSIX sh has no floating point, and this does the same rather than
# formatting a double: the two installers then agree to the last digit on every
# size, and no culture can turn the decimal point into a comma.
function Format-ByteSize {
    param([long]$Bytes)

    if ($Bytes -lt 0) { return '' }
    if ($Bytes -lt 1024) { return ([string]$Bytes + ' B') }

    # The steps below are install.sh's format_bytes, in the same order and with
    # the same unit ladder. "Agree to the last digit" only holds if the rounding
    # and the choice of unit are both the same, and they are the two things that
    # are easy to get subtly different: truncating the tenth reports 1100 bytes
    # as 1.0 KB where the shell says 1.1 KB, and settling the unit before the
    # tenth is rounded reports 1048575 bytes as 1023.9 KB where the shell says
    # 1.0 MB.
    $units = @('KB', 'MB', 'GB')
    $index = 0
    [long]$div = 1024
    [long]$whole = 0
    [long]$frac = 0

    while ($true) {
        # Every division here subtracts the remainder first. A plain [long] cast
        # rounds to nearest rather than truncating, so [long](15 / 10) is 2, and
        # 1536 bytes would be reported as 2.5 KB. Taking the remainder out first
        # makes the division exact, which is also what the shell arithmetic in
        # install.sh does.
        $rem = $Bytes % $div
        $whole = ($Bytes - $rem) / $div

        # A tenth of the unit, and half of that. The remainder is divided by the
        # tenth rather than multiplied by ten, because a gigabyte-sized multiply
        # overflows the signed long the shell side is only guaranteed to give
        # us, and adding half a tenth first is what makes this round.
        $tenth = ($div - ($div % 10)) / 10
        $half = ($div - ($div % 20)) / 20
        $scaled = $rem + $half
        $frac = ($scaled - ($scaled % $tenth)) / $tenth
        if ($frac -ge 10) {
            $whole = $whole + 1
            $frac = 0
        }

        # The unit is picked after rounding, not before. 1048575 bytes is
        # 1023.99 KB, which rounds to "1024.0 KB": a number the next unit up
        # exists precisely to avoid printing.
        if ($whole -lt 1024 -or $index -ge ($units.Count - 1)) { break }
        $div = $div * 1024
        $index = $index + 1
    }

    return ('{0}.{1} {2}' -f $whole, $frac, $units[$index])
}

# Paths are long on Windows and the home directory prefix carries no
# information, so the console shortens it the way a shell prompt would. The log
# keeps the full path everywhere.
function Get-DisplayPath {
    param([string]$Path)

    if ([string]::IsNullOrEmpty($Path)) { return '' }
    # Not $home: that name is a PowerShell automatic variable, and shadowing it
    # inside a function is a trap for whoever edits this next.
    $userHome = Get-UserHome
    if ([string]::IsNullOrEmpty($userHome)) { return $Path }
    if ([string]::Equals($Path, $userHome, [System.StringComparison]::OrdinalIgnoreCase)) { return '~' }

    $prefix = $userHome.TrimEnd('\', '/') + '\'
    if ($Path.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        return ('~\' + $Path.Substring($prefix.Length))
    }
    return $Path
}

# The download base without its scheme, which is what the source line shows. An
# http base keeps its scheme: a base that is not https is worth noticing.
function Get-SourceText {
    param([string]$Url)

    if ($Url.StartsWith('https://', [System.StringComparison]::OrdinalIgnoreCase)) {
        return $Url.Substring(8)
    }
    return $Url
}

function Get-SecondsText {
    param([System.Diagnostics.Stopwatch]$Timer)

    return $Timer.Elapsed.TotalSeconds.ToString('0.0', [System.Globalization.CultureInfo]::InvariantCulture)
}

function Get-CanonicalPath {
    param([string]$Path)

    if ([string]::IsNullOrEmpty($Path)) { return '' }
    try {
        return [System.IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    } catch {
        return $Path.TrimEnd('\', '/')
    }
}

# True when $Candidate is $Root itself or lives underneath it.
function Test-PathUnder {
    param([string]$Candidate, [string]$Root)

    $candidatePath = Get-CanonicalPath $Candidate
    $rootPath = Get-CanonicalPath $Root
    if ([string]::IsNullOrEmpty($candidatePath) -or [string]::IsNullOrEmpty($rootPath)) { return $false }
    if ([string]::Equals($candidatePath, $rootPath, [System.StringComparison]::OrdinalIgnoreCase)) { return $true }
    return $candidatePath.StartsWith($rootPath + [System.IO.Path]::DirectorySeparatorChar,
        [System.StringComparison]::OrdinalIgnoreCase)
}

function Invoke-NativeCapture {
    param([string]$FilePath, [string[]]$Arguments)

    # A native command writing to stderr can raise a terminating error while
    # $ErrorActionPreference is Stop. This assignment is function scoped, so it
    # relaxes the setting for this call only.
    $ErrorActionPreference = 'Continue'
    Write-Diagnostic ('running: ' + $FilePath + ' ' + ($Arguments -join ' '))
    $output = & $FilePath @Arguments 2>&1
    $code = Get-LastExitCode
    if ($null -eq $output) {
        Write-Diagnostic ('exit code ' + $code + ', no output')
        return ''
    }
    $text = ($output | Out-String).Trim()
    Write-Diagnostic ('exit code ' + $code + ', output: ' + $text)
    return $text
}

function Invoke-NativeCommand {
    param([string]$FilePath, [string[]]$Arguments)

    # Two problems solved here. Native tools report progress on stderr, which can
    # surface as a terminating error while $ErrorActionPreference is Stop. And
    # whatever a native command writes inside a function becomes part of that
    # function's return value, so a caller doing "$path = Install-Something"
    # would collect the entire build log. Printing through Write-Host keeps the
    # success stream empty and leaves only the exit code to report.
    $ErrorActionPreference = 'Continue'
    Write-LogLine 'INFO' ('running: ' + $FilePath + ' ' + ($Arguments -join ' '))
    # The streamed output stays on the console and out of the log on purpose.
    # install.sh cannot tee a build log into the file without pipefail, which
    # POSIX sh does not have, and two logs that match are worth more than one
    # longer one. The message after a failure points at the console output.
    & $FilePath @Arguments 2>&1 | ForEach-Object { Write-Host $_ }
    $code = Get-LastExitCode
    Write-LogLine 'INFO' ('exit code ' + $code + ' from ' + $FilePath)
    return $code
}

function Test-CommandExists {
    param([string]$Name)

    $command = Get-Command -Name $Name -ErrorAction SilentlyContinue
    return ($null -ne $command)
}

# ---------------------------------------------------------------------------
# Versions
# ---------------------------------------------------------------------------

function ConvertTo-VersionParts {
    param([string]$Text)

    $clean = $Text
    $cut = $clean.IndexOfAny([char[]]@('-', '+'))
    if ($cut -ge 0) { $clean = $clean.Substring(0, $cut) }

    $parts = @(0, 0, 0)
    $fields = $clean.Split('.')
    for ($i = 0; $i -lt 3; $i++) {
        if ($i -lt $fields.Length) {
            $parsed = 0
            if ([int]::TryParse($fields[$i], [ref]$parsed)) { $parts[$i] = $parsed }
        }
    }
    # The leading comma stops PowerShell unrolling the array on return.
    return ,$parts
}

function Compare-VersionNumber {
    param([string]$Left, [string]$Right)

    # Field by field numeric compare. A string compare would rank 1.100.0 below
    # 1.93.0 and 1.9.0 above it, and both answers are wrong.
    $leftParts = ConvertTo-VersionParts $Left
    $rightParts = ConvertTo-VersionParts $Right
    for ($i = 0; $i -lt 3; $i++) {
        if ($leftParts[$i] -gt $rightParts[$i]) { return 1 }
        if ($leftParts[$i] -lt $rightParts[$i]) { return -1 }
    }
    return 0
}

function ConvertTo-PlainVersion {
    param([string]$Text)

    $value = $Text.Trim()
    if ($value.StartsWith($script:TagPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        $value = $value.Substring($script:TagPrefix.Length)
    }
    if ($value.StartsWith('v', [System.StringComparison]::OrdinalIgnoreCase)) {
        $value = $value.Substring(1)
    }
    return $value
}

function Test-VersionText {
    param([string]$Text)

    # The version is pasted straight into a download URL and into the archive
    # file name under the temp directory, so it has to stay one path segment.
    # Rejecting everything outside the semver character set keeps a stray
    # '..\..\' or a query string out of both, and turns a typo into a clear
    # message instead of a 404 or a file written somewhere unexpected.
    if ([string]::IsNullOrEmpty($Text)) { return $false }
    return ($Text -match '^[0-9A-Za-z][0-9A-Za-z.+-]*$')
}

# ---------------------------------------------------------------------------
# Platform detection
# ---------------------------------------------------------------------------

function Get-MachineArchitecture {
    # PROCESSOR_ARCHITECTURE describes the current process, not the machine: a
    # 32-bit shell, or an x64 shell emulated on an ARM64 device, reports x86 or
    # AMD64 for what is really an ARM64 box. Windows sets PROCESSOR_ARCHITEW6432
    # only in that emulated case and it holds the real machine architecture, so
    # it wins when present.
    #
    # Which source answered is recorded in $script:ArchitectureSource, so the
    # log can say how the machine was identified without this precedence being
    # written out a second time somewhere else and drifting.
    $native = Get-EnvValue 'PROCESSOR_ARCHITEW6432'
    if (-not [string]::IsNullOrEmpty($native)) {
        $script:ArchitectureSource = 'PROCESSOR_ARCHITEW6432, so this is an emulated shell'
        return $native.ToUpperInvariant()
    }

    # RuntimeInformation is authoritative where it exists, but the type is
    # missing on older .NET Framework hosts, hence the try.
    $osArchitecture = ''
    try {
        $osArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    } catch {
        $osArchitecture = ''
    }
    if (-not [string]::IsNullOrEmpty($osArchitecture)) {
        $script:ArchitectureSource = 'RuntimeInformation.OSArchitecture'
        return $osArchitecture.ToUpperInvariant()
    }

    $script:ArchitectureSource = 'PROCESSOR_ARCHITECTURE, no better source on this host'
    return (Get-EnvValue 'PROCESSOR_ARCHITECTURE').ToUpperInvariant()
}

function Get-TargetTriple {
    param([string]$Architecture)

    # switch over strings is case insensitive by default. An empty result means
    # no prebuilt archive exists, which is a source build and not an error.
    switch ($Architecture) {
        'AMD64' { return 'x86_64-pc-windows-msvc' }
        'X64'   { return 'x86_64-pc-windows-msvc' }
        'ARM64' { return 'aarch64-pc-windows-msvc' }
        default { return '' }
    }
}

# The raw values behind the one line the console prints. install.sh logs its
# uname output, its glibc versus musl verdict and how it reached it; this is the
# same record for Windows.
function Write-PlatformFacts {
    param([string]$Architecture, [string]$Target)

    $description = ''
    $osArchitecture = ''
    try {
        $description = [string][System.Runtime.InteropServices.RuntimeInformation]::OSDescription
        $osArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    } catch {
        # Both are absent on older .NET Framework hosts, which is itself worth
        # seeing in the log.
    }

    $osVersion = ''
    try {
        $osVersion = [string][Environment]::OSVersion.VersionString
    } catch {
        $osVersion = ''
    }

    Write-LogLine 'INFO' 'platform:'
    Write-Fact 'RuntimeInformation.OSDescription' $description
    Write-Fact 'RuntimeInformation.OSArchitecture' $osArchitecture
    Write-Fact 'Environment.OSVersion' $osVersion
    Write-Fact 'PROCESSOR_ARCHITECTURE' (Get-EnvValue 'PROCESSOR_ARCHITECTURE')
    Write-Fact 'PROCESSOR_ARCHITEW6432' (Get-EnvValue 'PROCESSOR_ARCHITEW6432')
    Write-Fact 'machine architecture' ($Architecture + ' (from ' + $script:ArchitectureSource + ')')
    # Spelled out rather than left absent, so a reader comparing this file with
    # an install.sh log does not go looking for a missing line.
    Write-Fact 'libc' 'not applicable on Windows, the glibc and musl check has no counterpart here'

    if ([string]::IsNullOrEmpty($Target)) {
        Write-Fact 'target triple' ('none, no prebuilt archive is published for Windows ' + $Architecture)
    } else {
        Write-Fact 'target triple' $Target
    }
}

function Get-RustupArchitecture {
    param([string]$Architecture)

    switch ($Architecture) {
        'AMD64' { return 'x86_64' }
        'X64'   { return 'x86_64' }
        'ARM64' { return 'aarch64' }
        'X86'   { return 'i686' }
        default { return 'x86_64' }
    }
}

function Get-RustupUrl {
    $rustArch = Get-RustupArchitecture (Get-MachineArchitecture)
    return ('https://static.rust-lang.org/rustup/dist/' + $rustArch + '-pc-windows-msvc/rustup-init.exe')
}

function Get-RustupCommand {
    # The Windows equivalent of the "curl https://sh.rustup.rs | sh" line
    # install.sh prints, as one pasteable command.
    # -UseBasicParsing for the same reason the installer's own calls use it: the
    # default path wants the Internet Explorer engine, which Server Core and
    # freshly imaged machines do not have.
    return ('Invoke-WebRequest ' + (Get-RustupUrl) + ' -OutFile rustup-init.exe -UseBasicParsing; .\rustup-init.exe -y')
}

# ---------------------------------------------------------------------------
# Usage
# ---------------------------------------------------------------------------

function Show-Usage {
    $usage = @'
proa installer

Installs the Proa CLI (the 'proa' and 'cargo-proa' binaries) from a published
release archive, or builds it from source when no prebuilt binary matches this
machine.

USAGE
    irm https://proa.so/install.ps1 | iex
    irm https://proa.so/install.ps1 -OutFile install.ps1; .\install.ps1 [OPTIONS]

    The one liner takes no options. Invoke-Expression evaluates this script as a
    statement list, so there is no command to bind arguments to and -Version is
    ignored without warning. Use the environment variables below on that path,
    or download the script first and run it with options.

OPTIONS
    -Version <VERSION>    Install this version instead of the latest release.
                          Accepts 0.1.0, v0.1.0 or proa-cli-v0.1.0.
    -To <DIR>             Install the binaries into DIR instead of
                          $PROA_HOME\bin (default %USERPROFILE%\.proa\bin).
    -DownloadBase <URL>   Fetch latest.json and the release archives from this
                          base URL instead of https://proa.so/downloads/cli.
                          This is how you install from a GitHub mirror, a fork
                          or a local test server. A trailing slash is ignored.
    -Repo <OWNER/NAME>    Build from this GitHub repository when a source build
                          is needed. Downloads never use it, they go through
                          -DownloadBase. Default: Proa-Labs/proa
    -NoModifyPath         Never change the user PATH. The installer still
                          prints what to add.
    -BuildFromSource      Skip the prebuilt download and run
                          'cargo install'. With no version, build Git HEAD;
                          a version pins the release or snapshot source.
                          Requires cargo and takes several minutes.
    -PrebuiltOnly         Fail if the matching release archive cannot be
                          downloaded. Never fall back to a source build.
    -Yes                  Assume yes for every prompt. Use this in CI.
    -VerboseLog           Also print the log file's DEBUG detail to the
                          console. Named -VerboseLog because -Verbose is a
                          PowerShell common parameter; setting
                          $VerbosePreference = 'Continue' works too.
    -Help                 Print this help and exit.

ENVIRONMENT
    PROA_VERSION            Same as -Version.
    PROA_HOME               Install root. Default %USERPROFILE%\.proa; binaries
                            go into %PROA_HOME%\bin.
    PROA_INSTALL_DIR        Explicit bin directory. Overrides PROA_HOME.
    PROA_DOWNLOAD_BASE      Same as -DownloadBase. Default
                            https://proa.so/downloads/cli.
    PROA_REPO               Same as -Repo. Source builds only, downloads use
                            PROA_DOWNLOAD_BASE.
    PROA_NO_MODIFY_PATH     Any non-empty value behaves like -NoModifyPath.
    PROA_BUILD_FROM_SOURCE  Any non-empty value behaves like -BuildFromSource.
    PROA_PREBUILT_ONLY      Any non-empty value behaves like -PrebuiltOnly.
    PROA_YES                Any non-empty value behaves like -Yes.
    PROA_VERBOSE            Any non-empty value behaves like -VerboseLog.
    PROA_LOG                Write the log here instead of the default path.
                            The exact value 'off' disables logging entirely.
    NO_COLOR                Any non-empty value disables coloured output.
                            Colour is off anyway when the output is redirected,
                            and the plain markers -> and ok replace the UTF-8
                            ones when the console cannot render them.

    Flags win over environment variables.

LOGGING
    Every run writes
        %TEMP%\proa-install-<UTC timestamp>-<pid>.log
    and prints the path when it finishes. The file holds everything the console
    shows plus the detail it does not: the raw platform values, every HTTP
    request and its status, the expected and computed checksums, what was
    unpacked and where it went, and the reason behind every fallback. Attach it
    when reporting a problem. Only the PROA_* and NO_COLOR variables above are
    recorded; the environment is never dumped.

WHAT IT DOES
    1. Detects your CPU and picks the matching release archive.
    2. Checks for cargo and rustc. Proa builds your project with cargo, so a
       missing toolchain stops the install; a toolchain older than Rust
       1.93.0 is reported as a warning.
    3. Downloads the archive and its SHA256SUMS file over HTTPS from
       PROA_DOWNLOAD_BASE and verifies the checksum before anything is
       unpacked.
    4. Installs 'proa' and 'cargo-proa', then adds the bin directory to your
       user PATH if it is not already there. The Machine scope is never
       touched.

    Machines with no prebuilt archive (32-bit, anything unrecognised) fall back
    to building from source automatically. -PrebuiltOnly turns every fallback
    into an error, which is useful in CI.

EXAMPLES
    irm https://proa.so/install.ps1 | iex
    $env:PROA_VERSION = '0.1.0'; irm https://proa.so/install.ps1 | iex
    $env:PROA_PREBUILT_ONLY = '1'; irm https://proa.so/install.ps1 | iex
    .\install.ps1 -PrebuiltOnly -NoModifyPath -Yes
    .\install.ps1 -To C:\tools\proa -NoModifyPath
    .\install.ps1 -VerboseLog

    On Linux and macOS use install.sh instead:
    curl -fsSL https://proa.so/install.sh | sh
'@
    Write-Host $usage
    # The usage text itself is not mirrored: it is the same paragraph in every
    # log and it is already in this file.
    Write-LogLine 'INFO' 'printed usage and exited'
}



# The version rustc reports, or an empty string when it cannot be read or does
# not parse. Console only: the raw lines are already in the log.
function Get-RustcVersion {
    $line = ''
    try {
        $line = Invoke-NativeCapture 'rustc' @('--version')
    } catch {
        return ''
    }
    $match = [regex]::Match($line, '^rustc\s+(\d+\.\d+\.\d+)')
    if (-not $match.Success) { return '' }
    return $match.Groups[1].Value
}

# Proa compiles templates with cargo, so a project cannot be built without a
# toolchain even though the CLI itself is a prebuilt binary.
#
# This prints the toolchain line of the facts block itself, because only it
# knows what to say. Anything it wants to warn about is held in
# $script:ToolchainNotes and printed by Show-ToolchainNotes once the block is
# complete, so a warning cannot land between two facts and split the column.
function Invoke-RustPreflight {
    param([bool]$AssumeYes, [bool]$SourceBuildRequested)

    if ((Test-CommandExists 'cargo') -and (Test-CommandExists 'rustc')) {
        Write-LogLine 'INFO' 'toolchain: cargo and rustc are both on PATH'
        Write-Fact 'cargo' (Get-CommandPath 'cargo')
        Write-Fact 'rustc' (Get-CommandPath 'rustc')

        $line = ''
        try {
            $line = Invoke-NativeCapture 'rustc' @('--version')
        } catch {
            $line = ''
        }
        Write-Fact 'rustc --version' $line

        # Captured for the log only. The console never showed it and still does
        # not, but a mismatched cargo and rustc pair explains a whole class of
        # later build failures.
        $cargoLine = ''
        try {
            $cargoLine = Invoke-NativeCapture 'cargo' @('--version')
        } catch {
            $cargoLine = ''
        }
        Write-Fact 'cargo --version' $cargoLine

        $match = [regex]::Match($line, '^rustc\s+(\d+\.\d+\.\d+)')
        if ($match.Success) {
            $found = $match.Groups[1].Value
            if ((Compare-VersionNumber $found $script:Msrv) -ge 0) {
                Write-LogLine 'INFO' ('MSRV check: ' + $found + ' >= ' + $script:Msrv + ', ok')
                Write-KeyValue 'toolchain' ('Rust ' + $found + ' (minimum ' + $script:Msrv + ')')
            } else {
                # Not fatal: the prebuilt CLI runs anywhere. The user's first
                # project build is what would fail.
                Write-LogLine 'INFO' ('MSRV check: ' + $found + ' < ' + $script:Msrv + ', too old')
                Write-KeyValue 'toolchain' ('Rust ' + $found + ' (minimum ' + $script:Msrv + ', too old)')
                $script:ToolchainNotes = @(
                    ('warning:Rust ' + $found + ' is older than the minimum supported version ' + $script:Msrv + '.'),
                    'note:The CLI will still install, but building a Proa project will fail.',
                    'note:Update with: rustup update stable')
            }
        } else {
            Write-LogLine 'INFO' 'MSRV check: skipped, the rustc version line did not parse'
            Write-KeyValue 'toolchain' 'Rust, version not reported'
            $script:ToolchainNotes = @(
                "warning:could not parse the output of 'rustc --version', skipping the version check")
        }
        return
    }

    Write-LogLine 'INFO' ('toolchain: cargo present = ' + (Test-CommandExists 'cargo') +
        ', rustc present = ' + (Test-CommandExists 'rustc'))
    Write-LogLine 'INFO' 'Proa builds your project with cargo, and no Rust toolchain was found'

    # Rust is a hard requirement rather than something this installer will
    # provision. Proa builds your project with cargo, so a CLI installed
    # without a toolchain cannot do the one thing it exists for, and adding a
    # toolchain to someone's machine is a decision they should make
    # deliberately instead of inheriting from a piped script.
    Write-KeyValue 'toolchain' 'none found'
    # Worth the length: this is the one failure a first-time user is most
    # likely to hit, and the fix is not guessable from "cargo is not
    # recognized". It names the cause, why Rust is not optional here, both
    # ways to install it, and the PATH gotcha that makes a fresh rustup
    # look broken.
    Stop-Install 'Rust is required, and no toolchain was found on this machine.' @(
        '',
        'Proa compiles your templates into your binary at build time, so a Proa',
        'project is an ordinary Rust crate that cargo builds. Without cargo and',
        'rustc there is nothing for the CLI to drive, so installing it now would',
        'leave you with a command that cannot build your project.',
        '',
        ('Install Rust (' + $script:Msrv + ' or newer), then run this installer again.'),
        '',
        '  Get started   https://rust-lang.org/learn/get-started/',
        '',
        '  Or install it directly:',
        ('    ' + (Get-RustupCommand)),
        '',
        'If you just installed Rust, cargo is not on this session''s PATH yet.',
        'Open a new PowerShell window and run this installer again.')
}

# The preflight's warnings, printed after the facts block rather than inside it.
# The 'warning:' and 'note:' prefixes are how a note carries its own style
# without needing a second array to hold it.
function Show-ToolchainNotes {
    if ($null -eq $script:ToolchainNotes) { return }
    if ($script:ToolchainNotes.Count -eq 0) { return }

    Write-Blank
    foreach ($note in $script:ToolchainNotes) {
        if ($note.StartsWith('warning:')) {
            Write-Warn $note.Substring(8)
        } else {
            Write-Note $note.Substring(5)
        }
    }
    $script:ToolchainNotes = @()
}

# ---------------------------------------------------------------------------
# Downloads
# ---------------------------------------------------------------------------

function New-TempDirectory {
    $path = Join-Path ([System.IO.Path]::GetTempPath()) ('proa-install-' + [System.Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $path -Force | Out-Null
    Write-Diagnostic ('created temp directory ' + $path)
    return $path
}

function Remove-ItemKeepingLog {
    param([string]$Path)

    $children = @(Get-ChildItem -LiteralPath $Path -Force -ErrorAction SilentlyContinue)
    foreach ($child in $children) {
        if (Test-PathUnder $script:LogPath $child.FullName) {
            if ($child -is [System.IO.DirectoryInfo]) { Remove-ItemKeepingLog $child.FullName }
            continue
        }
        Remove-Item -LiteralPath $child.FullName -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Remove-TempDirectory {
    param([string]$Path)

    if ([string]::IsNullOrEmpty($Path)) { return }
    if (-not (Test-Path -LiteralPath $Path)) { return }

    # The default log path is a sibling of this directory, never inside it, for
    # exactly this reason: a log removed by the cleanup is worse than no log,
    # because the run then looks like it worked. PROA_LOG can name any path
    # though, so check instead of trusting the layout.
    #
    # The test is on the path, not on $script:LogEnabled. Cleanup runs from a
    # finally block while a failure is still unwinding, and by then Stop-Install
    # has closed the writer, so keying this on "is the log open" would delete
    # the file on exactly the runs whose log matters.
    if (-not [string]::IsNullOrEmpty($script:LogPath) -and (Test-PathUnder $script:LogPath $Path)) {
        Write-Diagnostic ('the log file is inside ' + $Path + ', removing everything around it and keeping the log')
        Remove-ItemKeepingLog $Path
        return
    }

    Write-Diagnostic ('removing temp directory ' + $Path)
    Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction SilentlyContinue
}

# Windows PowerShell 5.1 and PowerShell 7 both document -PassThru on
# Invoke-WebRequest, and it is the only way to see the status code of a
# transfer that went to -OutFile. Probed rather than assumed, so a host that
# somehow lacks it logs "no status reported" instead of failing a download that
# actually worked.
function Test-DownloadPassThru {
    if ($script:PassThruProbe -eq 'yes') { return $true }
    if ($script:PassThruProbe -eq 'no') { return $false }

    $supported = $false
    try {
        $command = Get-Command -Name 'Invoke-WebRequest' -ErrorAction SilentlyContinue
        if ($null -ne $command) { $supported = $command.Parameters.ContainsKey('PassThru') }
    } catch {
        $supported = $false
    }

    if ($supported) { $script:PassThruProbe = 'yes' } else { $script:PassThruProbe = 'no' }
    return $supported
}

function Get-ResponseStatusText {
    param($Response)

    try {
        if ($null -eq $Response) { return '' }
        if (-not ($Response.PSObject.Properties.Name -contains 'StatusCode')) { return '' }
        $text = [string][int]$Response.StatusCode
        if ($Response.PSObject.Properties.Name -contains 'StatusDescription') {
            $description = [string]$Response.StatusDescription
            if (-not [string]::IsNullOrEmpty($description)) { $text = $text + ' ' + $description }
        }
        return $text
    } catch {
        return ''
    }
}

function Get-ErrorStatusText {
    param($ErrorRecord)

    # A WebException on Windows PowerShell 5.1 and an HttpResponseException on
    # PowerShell 7 both hang the answer off the exception and both spell it
    # StatusCode, so one lookup covers both. Everything here is best effort: a
    # DNS failure or a refused connection has no response at all.
    try {
        if ($null -eq $ErrorRecord) { return '' }
        $exception = $ErrorRecord.Exception
        if ($null -eq $exception) { return '' }
        if (-not ($exception.PSObject.Properties.Name -contains 'Response')) { return '' }
        $response = $exception.Response
        if ($null -eq $response) { return '' }
        if (-not ($response.PSObject.Properties.Name -contains 'StatusCode')) { return '' }
        $status = $response.StatusCode
        if ($null -eq $status) { return '' }
        return ([string][int]$status + ' ' + [string]$status)
    } catch {
        return ''
    }
}

# Returns $true on success and $false on any HTTP or transport error, so a
# missing asset can be a fallback condition rather than a failure. Both outcomes
# are logged with the full URL and whatever status could be obtained: for a
# broken install this is the single most useful thing in the file.
function Invoke-Download {
    param([string]$Uri, [string]$OutFile, [bool]$ShowProgress)

    # Invoke-WebRequest repaints a progress bar for every chunk on Windows
    # PowerShell 5.1, and rendering it can cost more time than the transfer
    # itself, so silence is the default. The assignment is function scoped and
    # undone on return.
    #
    # The release archive is the one transfer worth watching, and only when a
    # person is actually watching: it is megabytes rather than the few hundred
    # bytes latest.json and the checksum file weigh, and on a slow link the run
    # would otherwise look hung. Redirected output gets nothing, because progress
    # rendering in a CI log or a pipe is noise, and on 5.1 it is expensive noise.
    # This is the counterpart of the --progress-bar install.sh passes to curl.
    $ProgressPreference = 'SilentlyContinue'
    if ($ShowProgress -and (Test-Interactive) -and (Test-ConsoleOutput)) {
        $ProgressPreference = 'Continue'
    }
    Write-LogLine 'INFO' ('http GET ' + $Uri)
    $timer = [System.Diagnostics.Stopwatch]::StartNew()

    try {
        # -UseBasicParsing is required on 5.1, where the default path wants the
        # Internet Explorer engine and fails on hosts that never ran it. Redirects
        # are followed by default and a non-2xx status throws, which the caller
        # sees as $false.
        $status = ''
        if (Test-DownloadPassThru) {
            $response = Invoke-WebRequest -Uri $Uri -OutFile $OutFile -UseBasicParsing -UserAgent $script:UserAgent `
                -TimeoutSec $script:DownloadTimeoutSec -PassThru
            $status = Get-ResponseStatusText $response
        } else {
            Invoke-WebRequest -Uri $Uri -OutFile $OutFile -UseBasicParsing -UserAgent $script:UserAgent `
                -TimeoutSec $script:DownloadTimeoutSec
        }
        $timer.Stop()
        if ([string]::IsNullOrEmpty($status)) { $status = 'no status reported' }
        Write-LogLine 'INFO' ('http ' + $status + ' ' + $Uri + ', ' + (Get-FileSizeText $OutFile) +
            ' in ' + (Get-SecondsText $timer) + 's')
        return $true
    } catch {
        $timer.Stop()
        $status = Get-ErrorStatusText $_
        if ([string]::IsNullOrEmpty($status)) { $status = 'no status' }
        Write-LogLine 'WARN' ('http ' + $status + ' ' + $Uri + ' failed after ' + (Get-SecondsText $timer) +
            's: ' + $_.Exception.Message)
        return $false
    }
}

# Returns the latest published version, or an empty string when there is
# nothing to install, which the caller turns into a source build rather than an
# error. Every unusable answer lands there: a 404 (no proa-cli release yet, the
# state today), a 503 (the download service cannot reach GitHub, or is rate
# limited), a body that is not JSON, and a version this script will not put in a
# URL.
#
# This asks the download base rather than api.github.com/repos/<repo>/releases/latest
# on purpose, and that is a fix as well as a repoint: /releases/latest answers
# with the newest release of any crate in the monorepo, so a proa_core or rsjs
# tag would resolve to a version that has no CLI archives at all. latest.json is
# filtered to the proa-cli-v tag prefix, so it is right by construction.
function Resolve-LatestVersion {
    param([string]$DownloadBase)

    # Silenced unconditionally, unlike the archive download. This request runs
    # between two lines of the facts block, so a progress bar here would flash
    # across a block that is meant to be still, and on Windows PowerShell 5.1
    # rendering one costs more than fetching the few hundred bytes it is
    # reporting on. Function scoped, so it is undone on return.
    $ProgressPreference = 'SilentlyContinue'
    $url = $DownloadBase + '/latest.json'
    Write-LogLine 'INFO' ('http GET ' + $url)
    $timer = [System.Diagnostics.Stopwatch]::StartNew()

    $response = $null
    try {
        $response = Invoke-WebRequest -Uri $url -UseBasicParsing -UserAgent $script:UserAgent `
            -Headers @{ 'Accept' = 'application/json' } -TimeoutSec 30
        $timer.Stop()
    } catch {
        # Invoke-WebRequest throws on any non-2xx status, so the 404 and the 503
        # both arrive here alongside DNS and connection failures. They all mean
        # the same thing to the caller.
        $timer.Stop()
        $status = Get-ErrorStatusText $_
        if ([string]::IsNullOrEmpty($status)) { $status = 'no status' }
        Write-LogLine 'WARN' ('http ' + $status + ' ' + $url + ' failed after ' + (Get-SecondsText $timer) +
            's: ' + $_.Exception.Message)
        return ''
    }

    $status = Get-ResponseStatusText $response
    if ([string]::IsNullOrEmpty($status)) { $status = 'no status reported' }
    Write-LogLine 'INFO' ('http ' + $status + ' ' + $url + ' in ' + (Get-SecondsText $timer) + 's')

    # ConvertFrom-Json exists on Windows PowerShell 5.1, which -AsHashtable and
    # -Depth do not, so this is the one spelling that works everywhere. It
    # raises a terminating error on malformed input, and $ErrorActionPreference
    # is Stop by the time this runs, so the try is what keeps a truncated or
    # HTML body a fallback condition instead of an unhandled failure.
    $json = $null
    try {
        $json = $response.Content | ConvertFrom-Json
    } catch {
        Write-LogLine 'WARN' ('the response from ' + $url + ' did not parse as JSON: ' + $_.Exception.Message)
        return ''
    }
    if ($null -eq $json) {
        Write-LogLine 'WARN' ('the response from ' + $url + ' was empty')
        return ''
    }

    $version = ''
    # latest.json is one object. A base pointing at something else entirely, an
    # API release list say, answers with an array, and saying so beats the
    # "carried no version field" line the property test below would produce.
    # PowerShell unrolls a one-element array to that element on the way out of
    # ConvertFrom-Json, so that case reads as an object here; it costs nothing,
    # because the version it yields still has to pass the check further down.
    if ($json -is [System.Array]) {
        Write-LogLine 'WARN' ('the response from ' + $url + ' is a JSON array, not a release object')
        return ''
    }
    # Tested rather than read directly: Set-StrictMode turns a missing property
    # into a terminating error rather than a null.
    if ($json.PSObject.Properties.Name -contains 'version') {
        $version = ([string]$json.version).Trim()
    }
    if ([string]::IsNullOrEmpty($version)) {
        Write-LogLine 'WARN' 'latest.json carried no version field'
        return ''
    }

    # The version comes off the network and is about to become part of a
    # download URL and of a file name under the temp directory, so it gets the
    # same check as the -Version flag. Anything this does not recognise is
    # treated as "no usable release" and falls through to the source build.
    $plain = ConvertTo-PlainVersion $version
    if (-not (Test-VersionText $plain)) {
        Write-LogLine 'WARN' ("the version '" + $version + "' is not shaped like a release version, ignoring it")
        return ''
    }

    $tag = ''
    if ($json.PSObject.Properties.Name -contains 'tag') {
        $tag = ([string]$json.tag).Trim()
    }
    if ([string]::IsNullOrEmpty($tag)) {
        Write-LogLine 'INFO' ('latest.json reports version ' + $plain)
    } else {
        Write-LogLine 'INFO' ('latest.json reports tag ' + $tag + ', version ' + $plain)
    }
    return $plain
}

# ---------------------------------------------------------------------------
# Checksums
# ---------------------------------------------------------------------------

function Get-ExpectedHash {
    param([string]$SumsPath, [string]$FileName)

    $lines = Get-Content -LiteralPath $SumsPath
    foreach ($line in $lines) {
        $trimmed = $line.Trim()
        if ([string]::IsNullOrEmpty($trimmed)) { continue }

        $fields = $trimmed -split '\s+', 2
        if ($fields.Length -lt 2) { continue }

        $name = $fields[1].Trim()
        # sha256sum marks binary mode with an asterisk in front of the name.
        if ($name.StartsWith('*')) { $name = $name.Substring(1) }
        if ([string]::Equals($name, $FileName, [System.StringComparison]::OrdinalIgnoreCase)) {
            return $fields[0].Trim().ToLowerInvariant()
        }
    }
    return ''
}

function Confirm-Checksum {
    param([string]$ArchivePath, [string]$SumsPath, [string]$ArchiveName)

    $expected = Get-ExpectedHash $SumsPath $ArchiveName
    if ([string]::IsNullOrEmpty($expected)) {
        Write-LogLine 'ERROR' ('checksum verdict: ' + $ArchiveName + ' has no entry in ' + (Split-Path -Path $SumsPath -Leaf))
        Write-StepFail 'checksum' 'no entry in the checksum file'
        Stop-Install ($ArchiveName + ' has no entry in ' + (Split-Path -Path $SumsPath -Leaf)) @(
            '',
            'Refusing to install a binary that cannot be verified.')
    }

    $actual = (Get-FileHash -LiteralPath $ArchivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    # Both hashes are recorded even when they match, so a later report about a
    # different machine can be compared against a run that worked.
    Write-LogLine 'INFO' ('checksum expected: ' + $expected + ' (' + (Split-Path -Path $SumsPath -Leaf) + ')')
    Write-LogLine 'INFO' ('checksum computed: ' + $actual + ' (' + $ArchiveName + ')')

    if ($expected -ne $actual) {
        # A mismatch means these are not the published bytes. There is no safe
        # way to continue, so this is always fatal and never a warning.
        Write-LogLine 'ERROR' 'checksum verdict: mismatch'
        Write-StepFail 'checksum' 'sha256 does not match'
        # Both hashes in full. They are here to be compared against the published
        # SHA256SUMS by eye, and a truncated hash cannot be.
        #
        # Each element is wrapped in its own parentheses because PowerShell binds
        # the comma tighter than '+': without them, "a" + $x, "b" + $y is
        # "a" + ($x, "b") + $y, which silently folds the whole list into one
        # string.
        Stop-Install ('checksum mismatch for ' + $ArchiveName) @(
            (('{0,-10}' -f 'expected') + $expected),
            (('{0,-10}' -f 'received') + $actual),
            '',
            'The download is corrupt or has been tampered with.')
    }

    Write-LogLine 'INFO' 'checksum verdict: match'
    Write-StepOk 'checksum' 'sha256 verified'
}

# ---------------------------------------------------------------------------
# Installing files
# ---------------------------------------------------------------------------

function Copy-Binary {
    param([string]$Source, [string]$Destination)

    if (-not (Test-Path -LiteralPath $Source)) {
        Stop-Install ('expected ' + (Split-Path -Path $Source -Leaf) + ' in the archive but it is missing') @(
            '',
            ('Looked for it at ' + $Source))
    }

    try {
        Copy-Item -LiteralPath $Source -Destination $Destination -Force
        # From here on the failure block stops claiming that nothing was
        # installed, because something now has been.
        $script:FilesWritten = $true
        Write-LogLine 'INFO' ('installed ' + $Source + ' -> ' + $Destination + ', ' + (Get-FileSizeText $Destination))
    } catch {
        $failure = $_
        $processName = [System.IO.Path]::GetFileNameWithoutExtension($Destination)
        $details = @('')

        # Windows keeps an executable locked for as long as it runs, so an
        # upgrade over a running proa.exe fails where every other overwrite
        # succeeds. Naming the process turns a confusing "access is denied" into
        # something the reader can act on.
        $running = @(Get-Process -Name $processName -ErrorAction SilentlyContinue)
        if ($running.Count -gt 0) {
            $descriptions = @()
            foreach ($process in ($running | Select-Object -First 6)) {
                $descriptions += ($process.Name + ' (pid ' + $process.Id + ')')
            }
            $listed = $descriptions -join ', '
            if ($running.Count -gt 6) {
                $listed = $listed + ', and ' + ($running.Count - 6) + ' more'
            }
            $details += ($processName + ' is running right now: ' + $listed)
            $details += 'Windows locks an executable while it runs, so it cannot be replaced in place.'
            $details += 'Close it (a dev server, a "cargo proa" command, an editor task), then run this installer again.'
        } else {
            $details += 'Windows locks an executable while it runs, so close any running proa.exe (a dev server, a "cargo proa" command, an editor task).'
            $details += 'Otherwise check the directory permissions, or pass -To with a writable directory.'
        }

        $details += ''
        $details += $failure.Exception.Message
        Stop-Install ('could not write ' + $Destination) $details
    }
}

function Get-InstalledVersion {
    param([string]$ExePath)

    if (-not (Test-Path -LiteralPath $ExePath)) { return '' }
    try {
        $out = Invoke-NativeCapture $ExePath @('--version')
        if ((Get-LastExitCode) -ne 0) { return 'unknown' }
        # Anything that is not the expected "proa <version>" line is reported as
        # unknown rather than echoed, so a broken binary cannot put its error
        # text into the upgrade summary.
        $match = [regex]::Match($out, '^proa\s+(\S+)')
        if (-not $match.Success) { return 'unknown' }
        return $match.Groups[1].Value
    } catch {
        return 'unknown'
    }
}

function Set-PreviousVersion {
    param([string]$BinDir)

    $script:PreviousVersion = Get-InstalledVersion (Join-Path $BinDir 'proa.exe')
    if ([string]::IsNullOrEmpty($script:PreviousVersion)) {
        Write-LogLine 'INFO' ('no existing install in ' + $BinDir)
    } else {
        Write-LogLine 'INFO' ('existing install in ' + $BinDir + ': ' + $script:PreviousVersion)
    }
}

# ---------------------------------------------------------------------------
# PATH handling
# ---------------------------------------------------------------------------

function Test-PathContains {
    param([string]$PathValue, [string]$Directory)

    if ([string]::IsNullOrEmpty($PathValue)) { return $false }
    $needle = $Directory.Trim().TrimEnd('\', '/')

    foreach ($entry in ($PathValue -split ';')) {
        $candidate = $entry.Trim().TrimEnd('\', '/')
        if ([string]::IsNullOrEmpty($candidate)) { continue }
        # Windows paths are case insensitive, so the entries compare that way.
        if ([string]::Equals($candidate, $needle, [System.StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }
    return $false
}

function Update-SessionPathFromEnvironment {
    $machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($null -eq $machinePath) { $machinePath = '' }
    if ($null -eq $userPath) { $userPath = '' }

    $combined = $machinePath.TrimEnd(';')
    if (-not [string]::IsNullOrEmpty($userPath)) {
        if ([string]::IsNullOrEmpty($combined)) {
            $combined = $userPath.TrimEnd(';')
        } else {
            $combined = $combined + ';' + $userPath.TrimEnd(';')
        }
    }
    if (-not [string]::IsNullOrEmpty($combined)) {
        Write-Diagnostic 'PATH: re-read the persisted Machine and User values into this session'
        $env:Path = $combined
    }
}

function Test-UserPathUsesExpansion {
    # GetEnvironmentVariable expands %VAR% references before returning, and
    # SetEnvironmentVariable writes the result back as a literal string.
    # Rewriting a PATH built from %VAR% references would freeze today's values
    # and silently drop the indirection, so detect that and leave it alone.
    try {
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
        if ($null -eq $key) { return $false }
        try {
            $raw = $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            if ($null -eq $raw) { return $false }
            return ([string]$raw).Contains('%')
        } finally {
            $key.Close()
        }
    } catch {
        # No registry access, which happens on PowerShell 7 off Windows. The
        # caller then takes the ordinary path.
        return $false
    }
}

# Printed in the closing block rather than here, so the run still ends with one
# tidy "what to do next" section instead of two competing ones.
function Show-ManualPathInstructions {
    param([string]$Directory)

    # Single quoted format strings keep $env:Path literal in the printed advice.
    $sessionLine = '$env:Path = "{0};$env:Path"' -f $Directory
    $permanentLine = '[Environment]::SetEnvironmentVariable("Path", "{0};" + [Environment]::GetEnvironmentVariable("Path", "User"), "User")' -f $Directory

    Write-Note 'Add it to this terminal:'
    Write-Command $sessionLine
    Write-Blank
    Write-Note 'And to every future one:'
    Write-Command $permanentLine
}

function Update-UserPath {
    param([string]$Directory, [bool]$NoModify)

    if ($NoModify) {
        Write-LogLine 'INFO' 'PATH: left alone, this run was asked not to change it'
        if (-not (Test-PathContains $env:Path $Directory)) {
            $script:PathState = 'manual'
            Write-StepStart 'path' 'not changed, this run was asked to leave it alone'
        } else {
            # Only this session's PATH was examined, because reading the user
            # scope is not this branch's business. So the closing block promises
            # nothing about other terminals: for all this run knows, the entry
            # was exported by hand a minute ago.
            $script:PathState = 'session'
            Write-LogLine 'INFO' ('PATH: ' + $Directory + " is already on this session's PATH")
            Write-StepStart 'path' 'not changed, the directory is already on PATH'
        }
        return
    }

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($null -eq $userPath) { $userPath = '' }
    # Windows has no shell profile files to examine, so the User scope of the
    # environment is the counterpart of the .bashrc and .zshrc list install.sh
    # walks. The Machine scope is never read or written.
    Write-LogLine 'INFO' 'PATH: examined the User scope Path value (HKCU\Environment)'
    Write-Diagnostic ('PATH: User scope value before this run = ' + $userPath)

    if (Test-PathContains $userPath $Directory) {
        $script:PathState = 'present'
        Write-LogLine 'INFO' ('PATH: ' + $Directory + ' was already in the User scope, nothing written')
        Write-StepOk 'path' 'already in your user environment'
    } elseif (Test-UserPathUsesExpansion) {
        $script:PathState = 'manual'
        Write-LogLine 'INFO' 'PATH: User scope left unchanged, rewriting it would freeze its %VAR% references'
        Write-StepStart 'path' 'not changed, your user PATH is built from %VAR% references'
        Write-Warn 'rewriting a PATH that uses %VAR% would freeze the values it refers to, so this run left it alone'
    } else {
        $newPath = $Directory
        if (-not [string]::IsNullOrEmpty($userPath)) {
            # Prepended so a freshly installed proa wins over an older copy,
            # which is what the install.sh profile line does as well.
            $newPath = $Directory + ';' + $userPath.TrimEnd(';')
        }
        # User scope only. The Machine scope needs administrator rights and would
        # change PATH for every account on the computer.
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        $script:PathState = 'changed'
        Write-LogLine 'INFO' ('PATH: prepended ' + $Directory + ' to the User scope Path')
        Write-StepOk 'path' 'added to your user environment'
    }

    # Every process gets its own copy of PATH at startup, so this session needs
    # the same edit before it can see proa without being restarted.
    if (-not (Test-PathContains $env:Path $Directory)) {
        Write-Diagnostic ('PATH: prepended ' + $Directory + " to this session's PATH")
        $env:Path = $Directory + ';' + $env:Path
    }
}

# ---------------------------------------------------------------------------
# Verification and next steps
# ---------------------------------------------------------------------------

function Confirm-Installation {
    param([string]$ExePath)

    Write-Diagnostic ('verifying ' + $ExePath)
    if (-not (Test-Path -LiteralPath $ExePath)) {
        Stop-Install ('the install finished but ' + $ExePath + ' is missing')
    }

    $out = ''
    $failed = $false
    try {
        $out = Invoke-NativeCapture $ExePath @('--version')
        if ((Get-LastExitCode) -ne 0) { $failed = $true }
    } catch {
        $out = $_.Exception.Message
        $failed = $true
    }
    if ($failed -or [string]::IsNullOrEmpty($out)) {
        Stop-Install ($ExePath + ' was installed but does not run') @(
            '',
            ('It answered: ' + $out))
    }

    $current = $out
    if ($current.StartsWith('proa ')) { $current = $current.Substring(5) }
    # Carried to the closing headline, which names the version that actually
    # runs rather than the one this run set out to install.
    $script:InstalledVersion = $current

    if (-not [string]::IsNullOrEmpty($script:PreviousVersion)) {
        Write-LogLine 'INFO' ('upgraded ' + $script:PreviousVersion + ' -> ' + $current)
    } else {
        Write-LogLine 'INFO' ('installed ' + $out)
    }
}

# An older copy earlier on PATH would keep answering to "proa", which is
# confusing enough to be worth calling out. This has to run after the PATH
# update, not before: on a first install the target directory is not on PATH
# yet, so checking early would report every unrelated copy as the winner when
# the PATH update is about to put the new one in front of it.
function Show-PathShadowWarning {
    param([string]$ExePath)

    $onPath = Get-Command -Name 'proa' -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $onPath) {
        Write-Diagnostic 'PATH: no proa resolves on this session PATH yet'
        return
    }
    Write-Diagnostic ('PATH: proa resolves to ' + $onPath.Source)
    if ([string]::Equals($onPath.Source, $ExePath, [System.StringComparison]::OrdinalIgnoreCase)) { return }

    Write-Warn ('another proa is earlier on your PATH at ' + $onPath.Source + ' and will win until you reorder PATH')
}

# The last thing a successful run prints, apart from the log path the entry
# point adds underneath. One headline, what to do about PATH, one command worth
# copying, and where the documentation lives.
function Show-NextSteps {
    param([string]$Directory)

    # Confirm-Installation always fills this in on the way here. The fallback is
    # for a future caller that does not, and says less rather than something
    # wrong.
    $headline = 'proa is ready.'
    if (-not [string]::IsNullOrEmpty($script:InstalledVersion)) {
        $headline = 'proa ' + $script:InstalledVersion + ' is ready.'
    }

    Write-Blank
    Write-Headline $headline
    Write-Blank

    if ($script:PathState -eq 'manual') {
        Write-Note ($Directory + ' is not on PATH.')
        Write-Blank
        Show-ManualPathInstructions $Directory
    } elseif ($script:PathState -eq 'session') {
        # Nothing was written to the user environment and nothing was read from
        # it either, so this says only what it can see.
        Write-Note 'This terminal can run proa now.'
    } else {
        # Update-UserPath has already edited this process's own copy of PATH, so
        # the current terminal needs nothing. Everything started before this run
        # keeps the PATH it inherited at launch.
        Write-Note 'This terminal can run proa now. Other programs pick it up when they next start.'
    }

    Write-Blank
    Write-Note 'Then create a site:'
    Write-Command 'proa new site hello-proa --template marketing --tailwind --yes'
    Write-Blank
    Write-Closing 'Docs' 'https://proa.so/docs'
}

# ---------------------------------------------------------------------------
# Install paths
# ---------------------------------------------------------------------------

function Get-SourceVersionText {
    param([string]$VersionText, [bool]$BuildFromSource, [string]$Repository)
    if ($VersionText) { return $VersionText }
    if ($BuildFromSource) { return 'Git HEAD, as asked' }
    if ($Repository -ine 'Proa-Labs/proa') { return 'Git HEAD (custom repository)' }
    return 'latest crates.io release'
}

function Get-SourceInstallArguments {
    param([string]$Repository, [string]$VersionText, [bool]$BuildFromSource)

    $arguments = @('install', '--locked', 'proa-cli')
    if ($VersionText -like '*-main.*') {
        if ($VersionText -notmatch '-main\.([0-9a-fA-F]{40})(?:\.|$)') {
            Stop-Install 'main snapshot version must contain a full 40-character commit SHA'
        }
        return $arguments + @('--git', ('https://github.com/' + $Repository), '--rev', $Matches[1])
    }
    if ($Repository -ieq 'Proa-Labs/proa' -and ($VersionText -or -not $BuildFromSource)) {
        $arguments += @('--registry', 'crates-io')
        if ($VersionText) { $arguments += @('--version', ('=' + $VersionText)) }
        return $arguments
    }
    $arguments += @('--git', ('https://github.com/' + $Repository))
    if ($VersionText) { $arguments += @('--tag', ('proa-cli-v' + $VersionText)) }
    return $arguments
}

function Install-FromSource {
    param([string]$Repository, [string]$BinDir, [bool]$DirWasExplicit,
          [string]$VersionText, [bool]$BuildFromSource = $false)

    $cargoBin = Get-CargoBinDir

    if (-not (Test-CommandExists 'cargo')) {
        Stop-Install 'building from source needs cargo, and it is not on PATH' @(
            '',
            'Install Rust, then run this installer again:',
            ('  ' + (Get-RustupCommand)))
    }

    # cargo install always writes to cargo's own bin directory, so a requested
    # directory cannot be honoured on this path.
    $sameDir = [string]::Equals($cargoBin.TrimEnd('\', '/'), $BinDir.TrimEnd('\', '/'), [System.StringComparison]::OrdinalIgnoreCase)
    if ($DirWasExplicit -and -not $sameDir) {
        Write-Warn ('cargo installs into ' + $cargoBin + ', so the requested directory ' + $BinDir + ' was not used')
    }
    # Any existing install that matters here is cargo's, so re-read the previous
    # version from there before the build overwrites it.
    Set-PreviousVersion $cargoBin

    Write-LogSection 'install'
    Write-StepStart 'building' 'proa-cli from source, this takes several minutes'
    Write-Blank

    # Use Git's existing credential helpers and proxy configuration when available.
    $callerFetch = Get-EnvValue 'CARGO_NET_GIT_FETCH_WITH_CLI'
    if ([string]::IsNullOrEmpty($callerFetch) -and (Get-Command git -ErrorAction SilentlyContinue)) {
        $env:CARGO_NET_GIT_FETCH_WITH_CLI = 'true'
    }
    try {
        $arguments = @(Get-SourceInstallArguments $Repository $VersionText $BuildFromSource)
        if ($arguments -contains '--registry') {
            Write-KeyValue 'source' 'crates.io/proa-cli (built on this machine)'
        } else {
            Write-KeyValue 'source' ('github.com/' + $Repository + ' (built on this machine)')
        }
        Write-LogLine 'INFO' ('running: cargo ' + ($arguments -join ' '))
        $code = Invoke-NativeCommand 'cargo' $arguments
    } finally {
        if ([string]::IsNullOrEmpty($callerFetch)) {
            Remove-Item -LiteralPath 'Env:\CARGO_NET_GIT_FETCH_WITH_CLI' -ErrorAction SilentlyContinue
        }
    }
    if ($code -ne 0) {
        Stop-Install 'cargo install failed' @(
            '',
            'The compiler error is in the output above.')
    }

    # cargo wrote the binaries itself, so the failure block must stop saying
    # that nothing was installed.
    $script:FilesWritten = $true
    Write-Blank
    Write-StepOk 'installed' ('proa, cargo-proa into ' + (Get-DisplayPath $cargoBin))
    return (Join-Path $cargoBin 'proa.exe')
}

# Returns the path of the installed proa.exe, or an empty string when there is
# no archive to download and the caller should build from source instead.
function Install-FromRelease {
    param([string]$DownloadBase, [string]$VersionText, [string]$Target, [string]$BinDir)

    # Also covers the version latest.json reported, which is untrusted input as
    # far as this function is concerned.
    if (-not (Test-VersionText $VersionText)) {
        Stop-Install ('refusing to build a download URL from the version string ' + $VersionText) @(
            '',
            'A version looks like 0.1.0, v0.1.0 or proa-cli-v0.1.0.')
    }

    # The two file names are the release workflow's contract and are the same
    # whichever host serves them; only the base in front of them moved.
    $archiveName = 'proa-cli-' + $VersionText + '-' + $Target + '.zip'
    $sumsName = 'proa-cli-' + $VersionText + '-SHA256SUMS'
    $baseUrl = $DownloadBase + '/' + $VersionText

    $tempDir = New-TempDirectory
    # finally also runs when the pipeline is stopped with Ctrl+C, so an
    # interrupted download does not leave the temp directory behind.
    try {
        Write-Diagnostic ('release base url: ' + $baseUrl)

        $archivePath = Join-Path $tempDir $archiveName
        $sumsPath = Join-Path $tempDir $sumsName

        # The archive is the one transfer with its own progress bar, so this
        # call is the only one that asks for it. The line below is printed after
        # the transfer rather than before it, because the size is the useful
        # part and it is not known until then; while the bytes are moving,
        # Invoke-WebRequest's own progress is what the reader watches.
        if (-not (Invoke-Download ($baseUrl + '/' + $archiveName) $archivePath $true)) {
            Write-StepFail 'downloading' ('no archive at ' + $baseUrl + '/' + $archiveName)
            Write-LogLine 'INFO' ('no prebuilt asset for target ' + $Target + ' at version ' + $VersionText + ', falling back to source')
            return ''
        }
        $archiveSize = Format-ByteSize (Get-FileSizeBytes $archivePath)
        if ([string]::IsNullOrEmpty($archiveSize)) {
            Write-StepStart 'downloading' $archiveName
        } else {
            Write-StepStart 'downloading' ($archiveName + ' (' + $archiveSize + ')')
        }

        if (-not (Invoke-Download ($baseUrl + '/' + $sumsName) $sumsPath)) {
            Write-StepFail 'checksum' ($sumsName + ' is missing')
            Stop-Install ('downloaded ' + $archiveName + ' but its checksum file is missing') @(
                ('expected it at ' + $baseUrl + '/' + $sumsName),
                '',
                'Refusing to install a binary that cannot be verified.')
        }

        Confirm-Checksum $archivePath $sumsPath $archiveName

        $extractDir = Join-Path $tempDir 'extract'
        try {
            Expand-Archive -LiteralPath $archivePath -DestinationPath $extractDir -Force
        } catch {
            Stop-Install ('could not unpack ' + $archiveName) @(
                '',
                $_.Exception.Message)
        }

        $entries = @(Get-ChildItem -LiteralPath $extractDir -Recurse -File -ErrorAction SilentlyContinue)
        Write-LogLine 'INFO' ($archiveName + ' unpacked, ' + $entries.Count + ' file(s):')
        foreach ($entry in $entries) {
            $relative = $entry.FullName
            if ($relative.Length -gt $extractDir.Length) {
                $relative = $relative.Substring($extractDir.Length).TrimStart('\', '/')
            }
            Write-LogLine 'INFO' ('  ' + $relative + ' (' + $entry.Length + ' bytes)')
        }

        # The archive holds a single top-level proa-cli-<version>-<target>
        # directory. Hand-built archives may be flat, so accept that too.
        $sourceDir = Join-Path $extractDir ('proa-cli-' + $VersionText + '-' + $Target)
        if (-not (Test-Path -LiteralPath $sourceDir)) {
            $found = Get-ChildItem -LiteralPath $extractDir -Recurse -Filter 'proa.exe' -File -ErrorAction SilentlyContinue |
                Select-Object -First 1
            if ($null -ne $found) {
                $sourceDir = $found.DirectoryName
                Write-LogLine 'INFO' ('the archive is not laid out as expected, using ' + $sourceDir + ' where proa.exe was found')
            } else {
                $sourceDir = $extractDir
                Write-LogLine 'INFO' ('no proa.exe found in the archive tree, trying the archive root ' + $sourceDir)
            }
        }

        Write-LogSection 'install'
        Write-LogLine 'INFO' ('installing into ' + $BinDir)
        try {
            New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
        } catch {
            Stop-Install ('could not create ' + $BinDir) @(
                '',
                $_.Exception.Message)
        }

        Copy-Binary (Join-Path $sourceDir 'proa.exe') (Join-Path $BinDir 'proa.exe')
        Copy-Binary (Join-Path $sourceDir 'cargo-proa.exe') (Join-Path $BinDir 'cargo-proa.exe')
        Write-StepOk 'installed' ('proa, cargo-proa into ' + (Get-DisplayPath $BinDir))
    } finally {
        Remove-TempDirectory $tempDir
    }

    return (Join-Path $BinDir 'proa.exe')
}

# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

function Invoke-Main {
    param(
        [string]$VersionArg,
        [string]$ToArg,
        [string]$DownloadBaseArg,
        [string]$RepoArg,
        [bool]$NoModifyPathArg,
        [bool]$BuildFromSourceArg,
        [bool]$PrebuiltOnlyArg,
        [bool]$YesArg,
        [bool]$HelpArg
    )

    # Both settings are dynamically scoped, so setting them in this function
    # covers every function it reaches. They deliberately are not set at file
    # scope: a try block does not create a scope either, so anywhere outside a
    # function would leave the caller's interactive session in strict mode with
    # terminating errors after "irm ... | iex" returns.
    Set-StrictMode -Version Latest
    $ErrorActionPreference = 'Stop'

    if ($HelpArg) {
        Show-Usage
        return
    }

    if (-not (Test-IsWindows)) {
        Stop-Install 'this is the Windows installer' @(
            '',
            'On Linux and macOS run:',
            "  curl -fsSL https://proa.so/install.sh | sh")
    }

    # Flags win over environment variables, so each setting reads its parameter
    # first and falls back to the environment only when that is empty. Each one
    # is logged as it is resolved, with where the value came from, so a run that
    # stops early still leaves a record of what it was working with.
    #
    # These names, plus NO_COLOR, PROA_LOG and PROA_VERBOSE below, are the only
    # environment variables this installer reads or records. Nothing enumerates
    # the environment: a file people are asked to paste into a public issue must
    # never carry a credential, and CARGO_REGISTRY_TOKEN is a variable
    # users really do export.
    Write-LogLine 'INFO' 'settings in effect:'

    # PROA_DOWNLOAD_BASE and PROA_REPO are an easy pair to confuse, so they are
    # resolved next to each other and each logged with what it actually
    # controls. Everything downloaded comes from the base; the repository is
    # only ever the argument to 'cargo install --git' on the source fallback.
    # Trimmed before the emptiness tests, so a variable holding nothing but
    # spaces reads as unset and takes the default rather than failing the check
    # below with a value nobody can see.
    $downloadBase = $DownloadBaseArg.Trim()
    $downloadBaseSource = 'flag -DownloadBase'
    if ([string]::IsNullOrEmpty($downloadBase)) {
        $downloadBase = (Get-EnvValue 'PROA_DOWNLOAD_BASE').Trim()
        $downloadBaseSource = 'environment'
    }
    if ([string]::IsNullOrEmpty($downloadBase)) {
        $downloadBase = $script:DefaultDownloadBase
        $downloadBaseSource = 'default'
    }
    # Trailing slash stripped once, here, so every URL built from it has exactly
    # one separator no matter how the value was written.
    $downloadBase = $downloadBase.TrimEnd('/')
    Write-Setting 'PROA_DOWNLOAD_BASE' $downloadBase $downloadBaseSource
    Write-LogLine 'INFO' '    latest.json and the release archives are fetched from here'
    # -notmatch is case insensitive, so HTTPS:// passes too.
    if ($downloadBase -notmatch '^https?://\S+$') {
        Stop-Install ('-DownloadBase (or PROA_DOWNLOAD_BASE) expects an http or https URL, got ' + $downloadBase) @(
            '',
            ('For example: ' + $script:DefaultDownloadBase))
    }

    $repository = $RepoArg
    $repositorySource = 'flag -Repo'
    if ([string]::IsNullOrEmpty($repository)) {
        $repository = Get-EnvValue 'PROA_REPO'
        $repositorySource = 'environment'
    }
    if ([string]::IsNullOrEmpty($repository)) {
        $repository = $script:DefaultRepo
        $repositorySource = 'default'
    }
    $script:Repository = $repository
    Write-Setting 'PROA_REPO' $repository $repositorySource
    Write-LogLine 'INFO' '    source builds only, downloads do not use it'
    if ($repository -notmatch '^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$') {
        Stop-Install ('-Repo expects an owner/name slug, got ' + $repository) @(
            '',
            ('For example: ' + $script:DefaultRepo))
    }

    $requestedVersion = $VersionArg
    $versionSource = 'flag -Version'
    if ([string]::IsNullOrEmpty($requestedVersion)) {
        $requestedVersion = Get-EnvValue 'PROA_VERSION'
        $versionSource = 'environment'
    }
    if ([string]::IsNullOrEmpty($requestedVersion)) {
        Write-Setting 'PROA_VERSION' '' ''
    } else {
        Write-Setting 'PROA_VERSION' $requestedVersion $versionSource
        $requestedVersion = ConvertTo-PlainVersion $requestedVersion
        if (-not (Test-VersionText $requestedVersion)) {
            Stop-Install ('-Version (or PROA_VERSION) expects a version, got ' + $requestedVersion) @(
                '',
                'A version looks like 0.1.0, v0.1.0 or proa-cli-v0.1.0.')
        }
        Write-Diagnostic ('requested version normalised to ' + $requestedVersion)
    }

    $proaHome = Get-EnvValue 'PROA_HOME'
    $homeSource = 'environment'
    if ([string]::IsNullOrEmpty($proaHome)) {
        $proaHome = Join-Path (Get-UserHome) '.proa'
        $homeSource = 'default'
    }
    Write-Setting 'PROA_HOME' $proaHome $homeSource

    $dirWasExplicit = $false
    $binDir = $ToArg
    $binDirSource = 'flag -To'
    if ([string]::IsNullOrEmpty($binDir)) {
        $binDir = Get-EnvValue 'PROA_INSTALL_DIR'
        $binDirSource = 'environment'
    }
    if ([string]::IsNullOrEmpty($binDir)) {
        Write-Setting 'PROA_INSTALL_DIR' '' ''
        $binDir = Join-Path $proaHome 'bin'
    } else {
        $dirWasExplicit = $true
        Write-Setting 'PROA_INSTALL_DIR' $binDir $binDirSource
    }

    $noModifyPath = $NoModifyPathArg
    $noModifyPathSource = 'flag -NoModifyPath'
    if (-not $noModifyPath) {
        $noModifyPath = Test-EnvFlag 'PROA_NO_MODIFY_PATH'
        $noModifyPathSource = 'environment'
    }
    Write-SettingFlag 'PROA_NO_MODIFY_PATH' $noModifyPath $noModifyPathSource

    $buildFromSource = $BuildFromSourceArg
    $buildFromSourceSource = 'flag -BuildFromSource'
    if (-not $buildFromSource) {
        $buildFromSource = Test-EnvFlag 'PROA_BUILD_FROM_SOURCE'
        $buildFromSourceSource = 'environment'
    }
    Write-SettingFlag 'PROA_BUILD_FROM_SOURCE' $buildFromSource $buildFromSourceSource

    $prebuiltOnly = $PrebuiltOnlyArg
    $prebuiltOnlySource = 'flag -PrebuiltOnly'
    if (-not $prebuiltOnly) {
        $prebuiltOnly = Test-EnvFlag 'PROA_PREBUILT_ONLY'
        $prebuiltOnlySource = 'environment'
    }
    Write-SettingFlag 'PROA_PREBUILT_ONLY' $prebuiltOnly $prebuiltOnlySource

    if ($buildFromSource -and $prebuiltOnly) {
        Stop-Install '-BuildFromSource and -PrebuiltOnly cannot be combined' @()
    }

    $assumeYes = $YesArg
    $assumeYesSource = 'flag -Yes'
    if (-not $assumeYes) {
        $assumeYes = Test-EnvFlag 'PROA_YES'
        $assumeYesSource = 'environment'
    }
    Write-SettingFlag 'PROA_YES' $assumeYes $assumeYesSource

    Write-Setting 'PROA_VERBOSE' (Get-EnvValue 'PROA_VERBOSE') 'environment'
    Write-Setting 'PROA_LOG' (Get-EnvValue 'PROA_LOG') 'environment'
    Write-Setting 'NO_COLOR' (Get-EnvValue 'NO_COLOR') 'environment'
    Write-Diagnostic ('bin directory resolved to ' + $binDir)

    $architecture = Get-MachineArchitecture
    $target = Get-TargetTriple $architecture
    Write-LogSection 'platform'
    Write-PlatformFacts $architecture $target

    # From here the console prints the facts block: platform, toolchain, source,
    # version, in that order and in that column. The toolchain line comes from
    # the preflight, which is the only code that knows what to say about it, and
    # the version line waits for the release to be resolved, which is a silent
    # network call.
    Write-Headline 'Proa CLI installer'
    Write-Blank
    if ([string]::IsNullOrEmpty($target)) {
        Write-KeyValue 'platform' ('Windows ' + $architecture + ' (no prebuilt archive)')
    } else {
        Write-KeyValue 'platform' ('Windows ' + $architecture + ' (' + $target + ')')
    }

    Invoke-RustPreflight $assumeYes $buildFromSource
    Set-PreviousVersion $binDir

    # What the version line says about an install that is already there. The
    # console used to report this after the fact as "Upgraded: a -> b"; saying it
    # up front is the same information, before rather than after the work.
    $replaces = ''
    if (-not [string]::IsNullOrEmpty($script:PreviousVersion)) {
        $replaces = ' (replacing ' + $script:PreviousVersion + ')'
    }

    Write-LogSection 'download'

    $exePath = ''
    if ($buildFromSource) {
        Write-LogLine 'INFO' 'a source build was requested, skipping the prebuilt download'
        Write-KeyValue 'version' ((Get-SourceVersionText $requestedVersion $true $repository) + $replaces)
        Show-ToolchainNotes
        Write-Blank
        $exePath = Install-FromSource $repository $binDir $dirWasExplicit $requestedVersion $true
    } elseif ([string]::IsNullOrEmpty($target)) {
        if ($prebuiltOnly) {
            Stop-Install ('no prebuilt proa-cli archive is published for Windows ' + $architecture) @(
                '',
                'Remove -PrebuiltOnly to allow a source build.')
        }
        Write-LogLine 'INFO' ('no prebuilt asset for Windows ' + $architecture + ', falling back to source')
        if ([string]::IsNullOrEmpty($requestedVersion)) {
            $requestedVersion = Resolve-LatestVersion $downloadBase
        }
        Write-KeyValue 'version' ((Get-SourceVersionText $requestedVersion $false $repository) + $replaces)
        Show-ToolchainNotes
        Write-Blank
        Write-Note ('There is no prebuilt proa-cli archive for Windows ' + $architecture + ', so this builds from source.')
        Write-Blank
        $exePath = Install-FromSource $repository $binDir $dirWasExplicit $requestedVersion
    } else {
        Write-KeyValue 'source' (Get-SourceText $downloadBase)

        $resolvedVersion = $requestedVersion
        if ([string]::IsNullOrEmpty($resolvedVersion)) {
            Write-Diagnostic ('no version requested, asking ' + $downloadBase + '/latest.json for the latest release')
            $resolvedVersion = Resolve-LatestVersion $downloadBase
        } else {
            Write-Diagnostic ('installing the requested version ' + $resolvedVersion + ', latest.json is not consulted')
        }

        if ([string]::IsNullOrEmpty($resolvedVersion)) {
            if ($prebuiltOnly) {
                Stop-Install ('could not resolve a published proa-cli release from ' + (Get-SourceText $downloadBase)) @(
                    '',
                    'Remove -PrebuiltOnly to allow a source build.')
            }
            Write-LogLine 'INFO' ('no usable release at ' + $downloadBase + ', falling back to source')
            Write-KeyValue 'version' ((Get-SourceVersionText '' $false $repository) + ' (binary channel unavailable)')
            Show-ToolchainNotes
            Write-Blank
            Write-Note ('No published proa-cli release was found at ' + (Get-SourceText $downloadBase) + '.')
            Write-Note 'Either none has been published yet, or the download service is unreachable or rate limited.'
            Write-Blank
            $exePath = Install-FromSource $repository $binDir $dirWasExplicit $resolvedVersion
        } else {
            Write-LogLine 'INFO' ('installing proa-cli ' + $resolvedVersion + ' for ' + $target + ' from a release archive')
            Write-KeyValue 'version' ($resolvedVersion + $replaces)
            Show-ToolchainNotes
            Write-Blank
            $exePath = Install-FromRelease $downloadBase $resolvedVersion $target $binDir
            if ([string]::IsNullOrEmpty($exePath)) {
                if ($prebuiltOnly) {
                    Stop-Install ('no prebuilt proa-cli ' + $resolvedVersion + ' archive exists for ' + $target) @(
                        '',
                        ('Expected it below ' + $downloadBase + '/' + $resolvedVersion + '.'),
                        'Remove -PrebuiltOnly to allow a source build.')
                }
                Write-Note 'Falling back to a source build.'
                Write-Blank
                $exePath = Install-FromSource $repository $binDir $dirWasExplicit $resolvedVersion
            }
        }
    }

    Confirm-Installation $exePath
    $installedDir = Split-Path -Path $exePath -Parent
    Update-UserPath $installedDir $noModifyPath
    Show-PathShadowWarning $exePath
    Show-NextSteps $installedDir
}

# Everything above is a definition, so a truncated transfer cannot run half an
# install. This final block is the only code with an effect.
try {
    # The console first, because every message below is printed through it and
    # the log header records what it decided. Then verbosity, so even that
    # header records how it was decided, then the log itself, so every message
    # from here on has somewhere to go.
    Initialize-Console
    Set-Verbosity ([bool]$VerboseLog)
    Initialize-Log

    Invoke-Main -VersionArg $Version -ToArg $To -DownloadBaseArg $DownloadBase -RepoArg $Repo `
        -NoModifyPathArg ([bool]$NoModifyPath) -BuildFromSourceArg ([bool]$BuildFromSource) `
        -PrebuiltOnlyArg ([bool]$PrebuiltOnly) `
        -YesArg ([bool]$Yes) -HelpArg ([bool]$Help)

    Complete-Log 'success' 0
    Show-LogPath $false
} catch {
    # Stop-Install already printed the explanation, wrote the footer and named
    # the log before unwinding with this token, so swallow it rather than
    # trailing a PowerShell stack dump after a message that was meant to be the
    # last word. Anything else is a bug in the installer: record what it was,
    # close the file properly, and rethrow it intact.
    if ($_.Exception.Message -ne $script:AbortToken) {
        Write-LogLine 'ERROR' ('unhandled error: ' + $_.Exception.Message)
        try {
            Write-LogLine 'DEBUG' ('at ' + [string]$_.InvocationInfo.PositionMessage)
            Write-LogLine 'DEBUG' ('stack: ' + [string]$_.ScriptStackTrace)
        } catch {
            # An error record without position information is still worth
            # reporting; the message above is the part that matters.
        }
        Complete-Log 'failure' 1
        Show-LogPath $true
        Close-Log
        throw
    }
} finally {
    # Reached on the normal path and on "exit" alike, which PowerShell unwinds
    # through finally blocks. Close-Log is idempotent, so the abort path having
    # closed the file already costs nothing.
    Close-Log
}
