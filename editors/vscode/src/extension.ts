/**
 * Entry point of the RedSAT extension.
 *
 * Highlighting, snippets and the language configuration are declarative (package.json); this code
 * only adds the language client for `redsat lsp`. Activation never throws, so a missing or broken
 * server never takes the declarative features down with it.
 */
import * as vscode from 'vscode';
import { ServerManager } from './serverManager';

const OUTPUT_CHANNEL_NAME = 'RedSAT Language Server';

let manager: ServerManager | undefined;

export function activate(context: vscode.ExtensionContext): void {
    try {
        // A log channel: the language client writes leveled entries and takes its trace level
        // from this channel's log level.
        const output = vscode.window.createOutputChannel(OUTPUT_CHANNEL_NAME, { log: true });
        const runsOnUiSide = context.extension.extensionKind === vscode.ExtensionKind.UI;
        const serverManager = new ServerManager(context.globalState, output, {
            uiSideOfRemote: runsOnUiSide ? vscode.env.remoteName : undefined,
        });
        manager = serverManager;
        context.subscriptions.push(
            vscode.commands.registerCommand('redsat.restartServer', () => serverManager.restart('command')),
            vscode.workspace.onDidChangeConfiguration(event => {
                if (event.affectsConfiguration('redsat.server')) {
                    serverManager.onSettingsChanged();
                }
            }),
            serverManager,
            output,
        );
        void serverManager.start('activation');
    } catch (error) {
        console.error('RedSAT: could not set up the language client.', error);
    }
}

export function deactivate(): Promise<void> | undefined {
    return manager?.stop();
}
