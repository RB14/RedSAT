/**
 * Resolution of the command line that starts the RedSAT language server.
 *
 * This module deliberately does not depend on the `vscode` API so that it can be exercised from
 * plain Node. Its only side effects are short-lived probe processes.
 */
import { spawn } from 'node:child_process';

/** Value of the `redsat.server.useWsl` setting. */
export type WslMode = 'auto' | 'always' | 'never';

/** The user settings that decide how the server is started. */
export interface ServerSettings {
    /** The `redsat` executable (bare name looked up on PATH, or a path). */
    readonly path: string;
    readonly useWsl: WslMode;
}

/** A concrete way to spawn the server (`cp.spawn(command, args)`, no shell). */
export interface ServerLaunch {
    readonly command: string;
    readonly args: readonly string[];
    /** Whether the server runs inside WSL while VS Code runs on Windows. */
    readonly viaWsl: boolean;
}

export type LaunchResolution =
    | { readonly kind: 'ok'; readonly launch: ServerLaunch }
    | { readonly kind: 'unavailable'; readonly reason: string };

export type Log = (message: string) => void;

/** The argument that switches the `redsat` CLI into language-server mode. */
const LSP_SUBCOMMAND = 'lsp';
const DEFAULT_PATH = 'redsat';
const WSL_EXE = 'wsl.exe';

/** Spawning a missing executable fails within milliseconds; this only guards against hangs. */
const DIRECT_PROBE_TIMEOUT_MS = 5_000;
/** A cold WSL VM can take several seconds to boot before it runs the first command. */
const WSL_PROBE_TIMEOUT_MS = 30_000;

/**
 * Decides how to start the server for the given settings.
 *
 * - On every platform but Windows, `<path> lsp` is spawned directly (WSL remote windows run the
 *   extension on the Linux side, so this is the Linux binary).
 * - On Windows, `useWsl` chooses between the native executable and
 *   `wsl.exe -e bash -lc "exec <path> lsp"`; `auto` falls back to WSL only when the native
 *   executable cannot be spawned.
 *
 * Each candidate is probed first so that a missing server is reported as `unavailable` (with a
 * human-readable reason) instead of as a language-client start failure.
 */
export async function resolveServerLaunch(
    settings: ServerSettings,
    log: Log,
    platform: NodeJS.Platform = process.platform,
): Promise<LaunchResolution> {
    const path = settings.path.trim() || DEFAULT_PATH;
    const direct = directLaunch(path);
    const wslMode: WslMode = platform === 'win32' ? settings.useWsl : 'never';

    if (wslMode !== 'always') {
        log(`Probing the server executable: ${describeLaunch(direct)}`);
        const probe = await probeSpawn(path, ['--version'], DIRECT_PROBE_TIMEOUT_MS);
        if (probe.ok) {
            return { kind: 'ok', launch: direct };
        }
        const reason = `'${path}' could not be started (${probe.error})`;
        if (wslMode === 'never') {
            return { kind: 'unavailable', reason };
        }
        log(`${reason}; trying WSL instead.`);
    }

    const viaWsl = wslLaunch(path);
    log(`Probing the server executable inside WSL: ${describeLaunch(viaWsl)}`);
    const probe = await probeWsl(path);
    if (probe.ok) {
        return { kind: 'ok', launch: viaWsl };
    }
    if (!probe.notFound) {
        return { kind: 'unavailable', reason: `WSL could not be used (${probe.error})` };
    }
    const where = wslMode === 'auto' ? 'was found neither on the Windows PATH nor in WSL' : 'was not found in WSL';
    return { kind: 'unavailable', reason: `'${path}' ${where}` };
}

/** Renders a launch as a shell-like command line, for logs and messages. */
export function describeLaunch(launch: ServerLaunch): string {
    return [launch.command, ...launch.args].map(arg => (/[\s"]/.test(arg) ? JSON.stringify(arg) : arg)).join(' ');
}

function directLaunch(path: string): ServerLaunch {
    return { command: path, args: [LSP_SUBCOMMAND], viaWsl: false };
}

function wslLaunch(path: string): ServerLaunch {
    // A login shell puts ~/.cargo/bin on PATH; `exec` replaces bash so that the server is the
    // process wsl.exe waits for and that receives the client's shutdown.
    const script = `exec ${bashQuotePath(path)} ${LSP_SUBCOMMAND}`;
    return { command: WSL_EXE, args: ['-e', 'bash', '-lc', script], viaWsl: true };
}

/** Quotes one word for bash; words made only of safe characters are left bare. */
export function bashQuote(word: string): string {
    if (/^[A-Za-z0-9_@%+=:,./-]+$/.test(word)) {
        return word;
    }
    return `'${word.replace(/'/g, `'\\''`)}'`;
}

/**
 * Quotes a path for bash while keeping a leading `~` or `~/` unquoted, so that the shell still
 * performs tilde expansion (it does not expand a tilde-prefix containing quoted characters).
 */
export function bashQuotePath(path: string): string {
    if (path === '~') {
        return path;
    }
    if (path.startsWith('~/')) {
        const rest = path.slice(2);
        return rest === '' ? '~/' : `~/${bashQuote(rest)}`;
    }
    return bashQuote(path);
}

type ProbeResult = { readonly ok: true } | { readonly ok: false; readonly error: string };

/**
 * Checks that `command` can be spawned the same way the language client will spawn it
 * (`cp.spawn` without a shell). Success is the `spawn` event; the probe process is killed right
 * away because only its startability matters.
 */
function probeSpawn(command: string, args: readonly string[], timeoutMs: number): Promise<ProbeResult> {
    return new Promise(resolve => {
        let child: ReturnType<typeof spawn>;
        try {
            child = spawn(command, args, { stdio: 'ignore', windowsHide: true });
        } catch (error) {
            resolve({ ok: false, error: errorMessage(error) });
            return;
        }
        const timer = setTimeout(() => {
            child.kill();
            resolve({ ok: false, error: `no response within ${timeoutMs} ms` });
        }, timeoutMs);
        child.once('spawn', () => {
            clearTimeout(timer);
            child.kill();
            resolve({ ok: true });
        });
        child.once('error', error => {
            clearTimeout(timer);
            resolve({ ok: false, error: errorMessage(error) });
        });
    });
}

type WslProbeResult =
    | { readonly ok: true }
    | { readonly ok: false; readonly notFound: boolean; readonly error: string };

/** Checks, through the same login shell the server will use, that `path` resolves inside WSL. */
function probeWsl(path: string): Promise<WslProbeResult> {
    return new Promise(resolve => {
        let child: ReturnType<typeof spawn>;
        try {
            child = spawn(WSL_EXE, ['-e', 'bash', '-lc', `command -v ${bashQuotePath(path)}`], {
                stdio: 'ignore',
                windowsHide: true,
            });
        } catch (error) {
            resolve({ ok: false, notFound: false, error: errorMessage(error) });
            return;
        }
        let settled = false;
        const settle = (result: WslProbeResult) => {
            if (!settled) {
                settled = true;
                clearTimeout(timer);
                resolve(result);
            }
        };
        const timer = setTimeout(() => {
            child.kill();
            settle({ ok: false, notFound: false, error: `no response within ${WSL_PROBE_TIMEOUT_MS / 1000} s` });
        }, WSL_PROBE_TIMEOUT_MS);
        child.once('error', error => settle({ ok: false, notFound: false, error: errorMessage(error) }));
        child.once('exit', (code, signal) => {
            if (code === 0) {
                settle({ ok: true });
            } else if (code === 1 || code === 127) {
                // `command -v` exits 1 when the name does not resolve; bash uses 127 similarly.
                settle({ ok: false, notFound: true, error: `exit code ${code}` });
            } else {
                settle({ ok: false, notFound: false, error: signal ? `killed by ${signal}` : `wsl.exe exited with code ${code}` });
            }
        });
    });
}

function errorMessage(error: unknown): string {
    if (error instanceof Error) {
        const code = (error as NodeJS.ErrnoException).code;
        return code === 'ENOENT' ? 'not found' : error.message;
    }
    return String(error);
}
