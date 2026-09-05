🌐 Base Servers
IDK Host: https://idk.neurokaraoke.com
API Host: https://api.neurokaraoke.com

🔑 Authentication Flows
1. Discord OAuth2 + PKCE
- Use Case: Native desktop/mobile apps that can trigger a web browser.
    Step 1 (Authorize): Direct browser to Discord's portal.
        Requies a code verifier (64 random bytes, URL-safe Base64, no padding).
    Step 2 (Discord Token): Exchange code via POST https://discord.com.
    Step 3 (NeuroToken Swap): Swap Discord accessToken for NeuroKaraoke JWT.
        Endpoint: POST https://neurokaraoke.com
        Expected Output: { "token": "<JWT>" } (Accept variations: token, accessToken, or a raw JSON string).

2. Username / Password LoginUse Case: Generic client access, scripting, and headless automated environments.
    Endpoint: POST https://neurokaraoke.com
    Validation Rules: Accepts Username only (no email); password length must be ≥ 6 characters.
    Expected Output: 200 OK with { "token": "<JWT>" } (Fallback to accessToken if token is missing).

3. QR Device Login
- Use Case: Devices lacking text inputs (Smart TVs).
    Step 1: Open a session via POST https://neurokaraoke.com (requires explicit Content-Length: 0 header).
    Step 2: Poll GET https://neurokaraoke.com{sessionId} every 4 seconds or use SSE via statusStreamUrl until approved.

4. Pairing Code Device Linking
- Use Case: Moving sessions between authenticated peripherals.
    Step 1: Mint a single-use 6-character code (expires in 5 minutes) via POST https://neurokaraoke.com with a Bearer Token.
    Step 2: Redeem the code via POST https://neurokaraoke.com to receive the JWT.

📦 Payload Schema, Constraints & Utility
- Token Lifecycle: No refresh endpoint exists; re-run a login flow upon expiration. Decode tokens using unpadded URL-safe Base64.
- Guest Fallback: Send x-guest-id: <uuid> instead of a bearer token for unauthenticated tracking on specific routes like PUT /api/songs/playCount/{id}.
- Verification Endpoint: Use GET https://neurokaraoke.com with a Bearer Token as a token-validity probe.