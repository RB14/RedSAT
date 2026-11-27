/**
 * Lifecycle of the RedSAT language client: start, stop and restart (serialized), a status item
 * for `.rsat` editors, and a one-time friendly warning when the server cannot be found.
 */
import * as vscode from 'vscode';
import { LanguageClient, type LanguageClientOptions, type ServerOptions, State } from 'vscode-languageclient/node';
import { describeLaunch, resolveServerLaunch, type ServerLaunch, type WslMode } from './launch';

const CLIENT_ID = 'redsat';
const CLIENT_NAME = 'RedSAT Language Server';
const CONFIG_SECTION = 'redsat.server';
const RESTART_COMMAND = 'redsat.restartServer';
const SUPPRESS_WARNING_KEY = 'redsat.server.suppressUnavailableWarning';
const INSTALL_REMEDY =
    'For diagnostics, completion and hover, install the server by running ' +
    '"cargo install --path crates/redsat-cli" in the RedSAT repository, or set "redsat.server.path".';
/** Settings edits arrive in bursts (e.g. while typing a path); restart once they settle. */
const SETTINGS_DEBOUNCE_MS = 1_000;
/** How long the client waits for the server to acknowledge `shutdown` before killing it. */
const STOP_TIMEOUT_MS = 2_000;

/** What triggered a (re)start; decides whether an "unavailable" warning is shown. */
export type StartTrigger = 'activation' | 'command' | 'settings';

type Status =
    | { readonly kind: 'starting' }
    | { readonly kind: 'running' }
    | { readonly kind: 'stopped' }
    | { readonly kind: 'disabled' }
    | { readonly kind: 'unavailable'; readonly reason: string };

/** Options that describe where the extension host runs. */
export interface ServerManagerOptions {
    /**
     * Set to the remote name (e.g. `wsl`) when this copy of the extension runs on the local (UI)
     * side of a remote window. Its documents then have the `vscode-remote` scheme, which the
     * language client does not serve, so the server is not started.
     */
    readonly uiSideOfRemote?: string;
}

export class ServerManager implements vscode.Disposable {
    private client: LanguageClient | undefined;
    private clientListener: vscode.Disposable | undefined;
    private launch: ServerLaunch | undefined;
    /** Tail of the chain that serializes start/stop operations. */
    private pending: Promise<void> = Promise.resolve();
    private settingsTimer: ReturnType<typeof setTimeout> | undefined;
    private warnedThisSession = false;
    private disposed = false;
    private readonly status: vscode.LanguageStatusItem;

    constructor(
        private readonly globalState: vscode.Memento,
        private readonly output: vscode.LogOutputChannel,
        private readonly options: ServerManagerOptions = {},
    ) {
        this.status = vscode.languages.createLanguageStatusItem('redsat.server', { language: 'rsat' });
        this.status.name = CLIENT_NAME;
        this.setStatus({ kind: 'starting' });
    }

    start(trigger: StartTrigger): Promise<void> {
        return this.enqueue(() => this.doStart(trigger));
    }

    restart(trigger: StartTrigger): Promise<void> {
        return this.enqueue(async () => {
            await this.doStop();
            await this.doStart(trigger);
        });
    }

    stop(): Promise<void> {
        this.cancelSettingsRestart();
        return this.enqueue(() => this.doStop());
    }

    /** Restarts the server once `redsat.server.*` settings stop changing. */
    onSettingsChanged(): void {
        this.cancelSettingsRestart();
        this.settingsTimer = setTimeout(() => {
            this.settingsTimer = undefined;
            void this.restart('settings');
        }, SETTINGS_DEBOUNCE_MS);
    }

    dispose(): Promise<void> {
        const stopped = this.stop();
        this.disposed = true;
        this.status.dispose();
        return stopped;
    }

    private enqueue(operation: () => Promise<void>): Promise<void> {
        const next = this.pending.then(operation).catch(error => {
            this.output.error(`Unexpected language client error: ${errorMessage(error)}`);
        });
        this.pending = next;
        return next;
    }

    private cancelSettingsRestart(): void {
        if (this.settingsTimer !== undefined) {
            clearTimeout(this.settingsTimer);
            this.settingsTimer = undefined;
        }
    }

    private async doStart(trigger: StartTrigger): Promise<void> {
        if (this.disposed) {
            return;
        }
        const config = vscode.workspace.getConfiguration(CONFIG_SECTION);
        if (!config.get<boolean>('enabled', true)) {
            this.output.info('The language server is disabled (redsat.server.enabled is false).');
            this.setStatus({ kind: 'disabled' });
            if (trigger === 'command') {
                void vscode.window.showInformationMessage(
                    'The RedSAT language server is disabled by the "redsat.server.enabled" setting.',
                );
            }
            return;
        }

        const remote = this.options.uiSideOfRemote;
        if (remote !== undefined) {
            const reason = `the extension runs on the local side of this ${remote} window`;
            this.output.warn(`Language server not started: ${reason}.`);
            this.setStatus({ kind: 'unavailable', reason });
            this.warnUnavailable(
                reason,
                trigger,
                `For diagnostics, completion and hover, install the RedSAT extension in ${remote} too ` +
                    '(Extensions view: "Install in …").',
            );
            return;
        }

        this.setStatus({ kind: 'starting' });
        const resolution = await resolveServerLaunch(
            { path: config.get<string>('path', 'redsat'), useWsl: config.get<WslMode>('useWsl', 'auto') },
            message => this.output.info(message),
        );
        if (this.disposed) {
            return;
        }
        if (resolution.kind === 'unavailable') {
            this.output.warn(`Language server unavailable: ${resolution.reason}.`);
            this.setStatus({ kind: 'unavailable', reason: resolution.reason });
            this.warnUnavailable(resolution.reason, trigger);
            return;
        }

        const launch = resolution.launch;
        this.output.info(`Starting the language server: ${describeLaunch(launch)}`);
        const client = this.createClient(launch);
        this.client = client;
        this.launch = launch;
        this.clientListener = client.onDidChangeState(event => {
            if (this.client === client) {
                this.onClientState(event.newState);
            }
        });
        try {
            await client.start();
            this.setStatus({ kind: 'running' });
        } catch (error) {
            // The client has already logged the failure and shown an error notification.
            this.output.error(`The language server failed to start: ${errorMessage(error)}`);
            await this.doStop();
            this.setStatus({ kind: 'unavailable', reason: 'the server failed to start; see the output' });
        }
    }

    private async doStop(): Promise<void> {
        const client = this.client;
        this.client = undefined;
        this.launch = undefined;
        this.clientListener?.dispose();
        this.clientListener = undefined;
        if (client === undefined) {
            return;
        }
        try {
            await client.dispose(STOP_TIMEOUT_MS);
        } catch (error) {
            this.output.warn(`Stopping the language server failed: ${errorMessage(error)}`);
        }
    }

    private createClient(launch: ServerLaunch): LanguageClient {
        // No `transport`: with `TransportKind.stdio` the client would append `--stdio` to the
        // arguments, whereas the server is started as exactly `redsat lsp`.
        const serverOptions: ServerOptions = { command: launch.command, args: [...launch.args] };
        const clientOptions: LanguageClientOptions = {
            documentSelector: [
                { scheme: 'file', language: 'rsat' },
                { scheme: 'untitled', language: 'rsat' },
            ],
            outputChannel: this.output,
            // Fail once (the client then shows a single error) instead of prompting to retry.
            initializationFailedHandler: error => {
                this.output.error(`The language server failed to initialize: ${errorMessage(error)}`);
                return false;
            },
        };
        return new LanguageClient(CLIENT_ID, CLIENT_NAME, serverOptions, clientOptions);
    }

    private onClientState(state: State): void {
        switch (state) {
            case State.Starting:
                this.setStatus({ kind: 'starting' });
                break;
            case State.Running:
                this.setStatus({ kind: 'running' });
                break;
            case State.Stopped:
                this.setStatus({ kind: 'stopped' });
                break;
        }
    }

    private setStatus(status: Status): void {
        const item = this.status;
        item.busy = status.kind === 'starting';
        item.severity =
            status.kind === 'unavailable' || status.kind === 'stopped'
                ? vscode.LanguageStatusSeverity.Warning
                : vscode.LanguageStatusSeverity.Information;
        item.command = { title: 'Restart', command: RESTART_COMMAND };
        switch (status.kind) {
            case 'starting':
                item.text = 'RedSAT server starting';
                item.detail = undefined;
                break;
            case 'running':
                item.text = 'RedSAT server';
                item.detail = this.launch?.viaWsl ? 'running in WSL' : 'running';
                break;
            case 'stopped':
                item.text = 'RedSAT server stopped';
                item.detail = undefined;
                break;
            case 'disabled':
                item.text = 'RedSAT server disabled';
                item.detail = undefined;
                item.command = {
                    title: 'Settings',
                    command: 'workbench.action.openSettings',
                    arguments: [CONFIG_SECTION],
                };
                break;
            case 'unavailable':
                item.text = 'RedSAT server unavailable';
                item.detail = status.reason;
                item.command = { title: 'Retry', command: RESTART_COMMAND };
                break;
        }
    }

    /**
     * Shows the "server unavailable" warning at most once per session (again after a settings
     * change), unless the user chose "Don't Show Again". An explicit restart always reports.
     */
    private warnUnavailable(reason: string, trigger: StartTrigger, remedy = INSTALL_REMEDY): void {
        if (trigger !== 'command') {
            const suppressed = this.globalState.get<boolean>(SUPPRESS_WARNING_KEY, false);
            if (suppressed || (this.warnedThisSession && trigger !== 'settings')) {
                return;
            }
        }
        this.warnedThisSession = true;

        const openSettings = 'Open Settings';
        const showOutput = 'Show Output';
        const dontShowAgain = "Don't Show Again";
        const message =
            `RedSAT language server unavailable: ${reason}. Syntax highlighting and snippets still work. ${remedy}`;
        // Not awaited: the notification may stay open indefinitely and must not block restarts.
        void vscode.window.showWarningMessage(message, openSettings, showOutput, dontShowAgain).then(choice => {
            if (choice === openSettings) {
                void vscode.commands.executeCommand('workbench.action.openSettings', CONFIG_SECTION);
            } else if (choice === showOutput) {
                this.output.show(true);
            } else if (choice === dontShowAgain) {
                void this.globalState.update(SUPPRESS_WARNING_KEY, true);
            }
        });
    }
}

function errorMessage(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
}
