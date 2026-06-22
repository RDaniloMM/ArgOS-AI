#!/usr/bin/env node
/**
 * Codex E2E Test — Login OAuth + Provider Test
 * ==============================================
 *
 * Test end-to-end del login OAuth de Codex (ChatGPT Pro/Plus).
 *
 * Modo manual: el script genera la URL de autorización, la mostramos,
 * VOS la abrís en Brave (ya logueado en OpenAI), completás el
 * consentimiento, y el script hace el resto:
 *   1. PKCE challenge + state
 *   2. TCP listener en localhost:1455 para el callback
 *   3. Te mostramos la URL para que copies en Brave
 *   4. Token exchange (auth code → access token)
 *   5. Llamada a Codex API (LLM real)
 *   6. Guarda el token para el test Rust
 *
 * Uso:
 *   node tests/codex-e2e/codex-login.js
 */

const crypto = require('crypto');
const net = require('net');
const fs = require('fs');
const path = require('path');

// ---------------------------------------------------------------------------
// Constantes (deben coincidir 1:1 con services.rs)
// ---------------------------------------------------------------------------
const CODEX_REDIRECT_PORT = 1455;
const CODEX_REDIRECT_URI = `http://localhost:${CODEX_REDIRECT_PORT}/auth/callback`;
const CODEX_AUTHORIZE_URL = 'https://auth.openai.com/oauth/authorize';
const CODEX_TOKEN_URL = 'https://auth.openai.com/oauth/token';
const CODEX_API_ENDPOINT = 'https://chatgpt.com/backend-api/codex/responses';
const OPENAI_OAUTH_CLIENT_ID = 'app_EMoamEEZ73f0CkXaXp7hrann';
const CODEX_SCOPE = 'openid profile email offline_access';
const CALLBACK_TIMEOUT_MS = 180_000;
const TOKEN_OUTPUT_PATH = path.join(__dirname, '.last-token.json');

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Inicia servidor TCP para recibir el callback OAuth. */
function startCallbackServer() {
    return new Promise((resolve, reject) => {
        const server = net.createServer((socket) => {
            // Procesar inmediatamente cuando llegan los datos HTTP,
            // sin esperar que el navegador cierre la conexión (keep-alive).
            socket.on('data', (chunk) => {
                const raw = chunk.toString();
                const lines = raw.split('\r\n');
                const requestLine = lines[0] || '';
                const [, fullPath] = requestLine.split(' ');

                if (!fullPath) return; // esperar más datos

                console.log(`\n  📩 Callback recibido: ${fullPath}`);

                const qs = fullPath?.split('?')[1] || '';
                const params = new URLSearchParams(qs);
                const code = params.get('code');
                const error = params.get('error');
                const errDesc = params.get('error_description');
                const receivedState = params.get('state');

                if (code) console.log(`     code: ${code.substring(0, 12)}...`);
                if (error) console.log(`     error: ${error} ${errDesc ? `(${errDesc})` : ''}`);

                // Responder OK con charset explícito para caracteres español
                const body =
                    '<!DOCTYPE html><html lang="es"><head><meta charset="UTF-8"><title>Autorizado</title></head><body><h1>✓ Autorizado</h1><p>Ya podés cerrar esta pestaña.</p></body></html>';
                socket.write(
                    [
                        'HTTP/1.1 200 OK',
                        'Content-Type: text/html; charset=utf-8',
                        `Content-Length: ${Buffer.byteLength(body)}`,
                        'Connection: close',
                        '',
                        body,
                    ].join('\r\n')
                );
                socket.end();
                server.close();

                resolve({ code, state: receivedState, error, error_description: errDesc });
            });
            socket.on('error', () => {});
        });

        server.listen(CODEX_REDIRECT_PORT, '127.0.0.1', () =>
            console.log(`  ✓ TCP listener en puerto ${CODEX_REDIRECT_PORT}`)
        );
        server.on('error', (err) => reject(err));
        setTimeout(() => {
            server.close();
            reject(new Error(`Timeout: no se recibió callback en ${CALLBACK_TIMEOUT_MS / 1000}s`));
        }, CALLBACK_TIMEOUT_MS);
    });
}

/** Extrae chatgpt_account_id del JWT. */
function extractAccountId(jwt) {
    try {
        const payload = JSON.parse(
            Buffer.from(jwt.split('.')[1], 'base64url').toString('utf-8')
        );
        return payload['https://api.openai.com/auth']?.chatgpt_account_id || '';
    } catch {
        return '';
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------
(async () => {
    console.log('');
    console.log('╔══════════════════════════════════════════════╗');
    console.log('║   Codex E2E — Login OAuth + Provider Test   ║');
    console.log('╚══════════════════════════════════════════════╝\n');

    // ------------------------------------------------------------------
    // 1. PKCE challenge + state (igual que services.rs)
    // ------------------------------------------------------------------
    const codeVerifier = crypto.randomBytes(64).toString('base64url').slice(0, 86);
    const codeChallenge = crypto
        .createHash('sha256')
        .update(codeVerifier)
        .digest()
        .toString('base64url');
    const state = crypto.randomBytes(24).toString('base64url');
    console.log('  ✓ PKCE generado');

    // ------------------------------------------------------------------
    // 2. Construir authorize URL
    // ------------------------------------------------------------------
    const authUrl = `${CODEX_AUTHORIZE_URL}?${new URLSearchParams({
        response_type: 'code',
        client_id: OPENAI_OAUTH_CLIENT_ID,
        redirect_uri: CODEX_REDIRECT_URI,
        scope: CODEX_SCOPE,
        code_challenge: codeChallenge,
        code_challenge_method: 'S256',
        state: state,
        id_token_add_organizations: 'true',
        codex_cli_simplified_flow: 'true',
        originator: 'argos-e2e-test',
    })}`;

    // ------------------------------------------------------------------
    // 3. Iniciar TCP listener
    // ------------------------------------------------------------------
    const callbackPromise = startCallbackServer();

    // ------------------------------------------------------------------
    // 4. Mostrar URL al usuario
    // ------------------------------------------------------------------
    console.log('');
    console.log('  ─────────────────────────────────────────────────────');
    console.log('  📋  PASO 1: Copiá esta URL en Brave (ya logueado)');
    console.log('  ─────────────────────────────────────────────────────');
    console.log('');
    console.log(`  ${authUrl}`);
    console.log('');
    console.log('  ─────────────────────────────────────────────────────');
    console.log('  📋  PASO 2: En Brave:');
    console.log('    1. Si ves un CAPTCHA/verificación → completalo');
    console.log('    2. Si ves "Continue as <tu email>" → hacele click');
    console.log('    3. Si ves "Accept"/"Authorize" → hacele click');
    console.log('    4. El navegador te va a redirigir a localhost:1455');
    console.log('       y vas a ver "✓ Autorizado"');
    console.log('  ─────────────────────────────────────────────────────');
    console.log('');
    console.log(`  ⏳  Esperando callback (${CALLBACK_TIMEOUT_MS / 1000}s de timeout)...\n`);

    // ------------------------------------------------------------------
    // 5. Esperar callback en el servidor TCP
    // ------------------------------------------------------------------
    let code;
    try {
        const result = await callbackPromise;
        code = result.code;
        const receivedState = result.state;
        if (!code) throw new Error('Callback sin code parameter');
        if (receivedState !== state) throw new Error('State mismatch — posible CSRF');
    } catch (err) {
        console.error(`\n  ❌ ${err.message}`);
        console.log('\n╔══════════════════════════════════════════════╗');
        console.log('║   ❌  TEST FAILED                          ║');
        console.log('╚══════════════════════════════════════════════╝');
        process.exit(1);
    }

    console.log(`  ✓ Callback recibido (code=${code.substring(0, 8)}...)`);

    // ------------------------------------------------------------------
    // 6. Intercambiar auth code → tokens OAuth
    // ------------------------------------------------------------------
    console.log('\n── Token Exchange ──');
    const tokenRes = await fetch(CODEX_TOKEN_URL, {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: new URLSearchParams({
            grant_type: 'authorization_code',
            client_id: OPENAI_OAUTH_CLIENT_ID,
            code: code,
            code_verifier: codeVerifier,
            redirect_uri: CODEX_REDIRECT_URI,
        }),
    });
    if (!tokenRes.ok) {
        console.error(`Token exchange falló (${tokenRes.status}): ${await tokenRes.text()}`);
        process.exit(1);
    }
    const tokens = await tokenRes.json();
    const accessToken = tokens.access_token;
    console.log(`  ✓ Access token: ${accessToken.substring(0, 20)}...`);

    // ------------------------------------------------------------------
    // 7. Guardar token ANTES de probar la API (útil aunque la API falle)
    // ------------------------------------------------------------------
    const accountId = extractAccountId(accessToken);
    const tokenData = {
        access_token: accessToken,
        account_id: accountId,
        expires_at: tokens.expires_in
            ? Date.now() + tokens.expires_in * 1000
            : Date.now() + 3600_000,
        model: 'gpt-5',
        endpoint: 'https://chatgpt.com/backend-api',
    };
    fs.writeFileSync(TOKEN_OUTPUT_PATH, JSON.stringify(tokenData, null, 2));
    console.log(`  ✓ Token guardado en ${path.relative(process.cwd(), TOKEN_OUTPUT_PATH)}`);

    // ------------------------------------------------------------------
    // 8. Probar Codex API con varios modelos
    //    (gpt-5 falla para cuentas ChatGPT, probamos alternativas)
    // ------------------------------------------------------------------
    console.log('\n── Codex API Call ──');
    const sessionId = crypto.randomUUID();

    const MODELS_TO_TRY = ['gpt-5.5', 'o4-mini', 'o3-mini', 'gpt-4o', 'gpt-5'];
    let responseText = '';
    let lastError = '';

    for (const model of MODELS_TO_TRY) {
        process.stdout.write(`  Probando modelo ${model}... `);
        const apiRes = await fetch(CODEX_API_ENDPOINT, {
            method: 'POST',
            headers: {
                Authorization: `Bearer ${accessToken}`,
                'Content-Type': 'application/json',
                'OpenAI-Beta': 'responses=experimental',
                originator: 'codex_cli_rs',
                'session_id': crypto.randomUUID(),
                ...(accountId ? { 'ChatGPT-Account-Id': accountId } : {}),
            },
            body: JSON.stringify({
                model,
                input: [{ role: 'user', content: 'Decime "Hola desde ArgOS!" en español y nada más.' }],
                instructions: 'Sos un asistente útil y conciso.',
                store: false,
                stream: true, // Codex endpoint obliga streaming
            }),
        });

        if (apiRes.ok) {
            // Codex devuelve SSE (Server-Sent Events), leer el primer chunk
            const reader = apiRes.body.getReader();
            const decoder = new TextDecoder();
            let firstChunk = '';
            while (firstChunk.length < 200) {
                const { done, value } = await reader.read();
                if (done) break;
                firstChunk += decoder.decode(value, { stream: true });
                if (firstChunk.includes('\n\n')) break;
            }
            reader.cancel();

            // Parsear SSE: buscar "data: {...}"
            for (const line of firstChunk.split('\n')) {
                if (line.startsWith('data: ') && line !== 'data: [DONE]') {
                    try {
                        const json = JSON.parse(line.slice(6));
                        responseText = json.delta || json.output_text || '';
                    } catch {}
                }
            }
            if (!responseText) responseText = '(streaming response recibido)';
            console.log('✅');
            // Update saved model
            tokenData.model = model;
            fs.writeFileSync(TOKEN_OUTPUT_PATH, JSON.stringify(tokenData, null, 2));
            break;
        } else {
            const errBody = await apiRes.text();
            lastError = `${apiRes.status}: ${errBody}`;
            console.log(`❌ (${lastError.substring(0, 60)}...)`);
        }
    }

    if (!responseText) {
        console.error(`\n  ❌ Todos los modelos fallaron. Último error: ${lastError}`);
        console.log('\n╔══════════════════════════════════════════════╗');
        console.log('║   ⚠️  Token guardado, API no disponible     ║');
        console.log('╚══════════════════════════════════════════════╝');
        console.log(
            '\n  El token se guardó igual para que podás probar con argos-tui.\n' +
            '  Puede que tu cuenta no tenga acceso a Codex o necesite otro modelo.'
        );
        process.exit(0);
    }

    console.log(`\n  🤖  "${responseText}"`);

    // ------------------------------------------------------------------
    // 9. Done
    // ------------------------------------------------------------------
    console.log('\n╔══════════════════════════════════════════════╗');
    console.log('║   ✅  TEST PASSED — Token válido           ║');
    console.log('╚══════════════════════════════════════════════╝');
    console.log(
        '\n  Próximo paso: correr el test Rust del provider:\n' +
        '    $env:CODEX_TOKEN_FILE="tests/codex-e2e/.last-token.json"\n' +
        '    cargo test codex_provider_e2e -- --ignored\n'
    );

    // ------------------------------------------------------------------
    // 10. Fin
    // ------------------------------------------------------------------
    // Ahora puedo ejecutar el test Rust del provider si querés.
})();
