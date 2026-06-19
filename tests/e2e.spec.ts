// Zenithar end-to-end tests (Playwright). Run via `make e2e`, which starts an
// ephemeral server on a temp DB and passes BASE + ADMIN_LINK.
//
// The browser specs drive the real UI (so they catch issues a fetch-level test
// misses, e.g. assets failing to load under /i/:token). The API spec covers the
// auth gate, revocation, and logout via Playwright request contexts.
import { test, expect } from "@playwright/test";

const ADMIN_LINK = process.env.ADMIN_LINK!;
const composer = /Написать|Write a message/i; // message placeholder (RU default / EN)

test.describe.configure({ mode: "serial" });

test("browser: link login renders the chat and sends a message", async ({ page }) => {
  await page.goto(ADMIN_LINK);

  // The token is dropped from the URL after the cookie is set.
  await expect(page).toHaveURL(/\/$/);
  // App actually mounted (would be blank if assets failed to load). Employees
  // see the room switcher in the header (which doubles as the brand slot).
  await expect(page.getByRole("button", { name: /Чаты|Chats/ })).toBeVisible();

  const input = page.getByPlaceholder(composer);
  await expect(input).toBeVisible();

  // Wait for the WebSocket to be live before sending (send is a no-op otherwise).
  await expect(page.locator('.beacon[data-state="live"]')).toBeVisible({ timeout: 10000 });

  const msg = `pw-${Date.now()}`;
  await input.fill(msg);
  await input.press("Enter");
  await expect(page.getByText(msg)).toBeVisible();
});

test("browser: a sent message survives a reload (persisted history)", async ({ page }) => {
  await page.goto(ADMIN_LINK);
  await expect(page.locator('.beacon[data-state="live"]')).toBeVisible({ timeout: 10000 });

  const msg = `persist-${Date.now()}`;
  const input = page.getByPlaceholder(composer);
  await input.fill(msg);
  await input.press("Enter");
  await expect(page.getByText(msg)).toBeVisible();

  // Let the batched writer commit (≤50ms) before reloading, otherwise the
  // reload could race the persistence and read history that's still mid-flush.
  await page.waitForTimeout(300);

  // Reload: the message must come back from server-side history, not live WS.
  await page.reload();
  await expect(page.getByText(msg)).toBeVisible({ timeout: 10000 });
});

test("browser: reply quotes the original and jumps back to it", async ({ page }) => {
  await page.goto(ADMIN_LINK);
  await expect(page.locator('.beacon[data-state="live"]')).toBeVisible({ timeout: 10000 });

  const orig = `orig-${Date.now()}`;
  const input = page.getByPlaceholder(composer);
  await input.fill(orig);
  await input.press("Enter");
  await expect(page.getByText(orig)).toBeVisible();

  // Open the message menu (desktop: click the message): it offers Reply + Copy.
  await page.getByText(orig).click();
  await expect(page.getByRole("menuitem", { name: /Копировать|Copy/ })).toBeVisible();
  await page.getByRole("menuitem", { name: /Ответить|Reply/ }).click();

  const reply = `reply-${Date.now()}`;
  await input.fill(reply);
  await input.press("Enter");
  await expect(page.getByText(reply)).toBeVisible();

  // The reply message renders a quote containing the original text.
  const replyRow = page.locator(".line", { hasText: reply });
  await expect(replyRow.getByText(orig)).toBeVisible();

  // Clicking the quote flashes the original row.
  await replyRow.getByText(orig).click();
  await expect(page.locator(".line.flash")).toBeVisible();
});

test("browser: admin self-rename sticks", async ({ page }) => {
  await page.goto(ADMIN_LINK);
  // exact: the header's name button (reply-quote buttons can also contain "admin").
  await page.getByRole("button", { name: "admin", exact: true }).click();
  const edit = page.getByLabel(/Изменить имя|Edit name/);
  await edit.fill("Chief");
  await edit.press("Enter");
  await expect(page.getByRole("button", { name: "Chief" })).toBeVisible();
});

test("browser: admin issues a client link and the client can open it", async ({
  page,
  browser,
}) => {
  await page.goto(ADMIN_LINK);
  await page.getByRole("button", { name: /Ссылки|Links/ }).click();
  // The "Create" button exists in both the links and integrations sections;
  // the first one is the link creator.
  await page.getByRole("button", { name: /Создать|Create/ }).first().click();

  const link = await page.locator("code").first().textContent();
  expect(link).toContain("/i/");

  // Open the link in a fresh context (separate cookie jar) = the client.
  const ctx = await browser.newContext();
  const clientPage = await ctx.newPage();
  await clientPage.goto(link!);
  await expect(clientPage.getByPlaceholder(composer)).toBeVisible();

  // The client must reach a live socket and be able to send.
  await expect(clientPage.locator('.beacon[data-state="live"]')).toBeVisible({ timeout: 10000 });
  const cinput = clientPage.getByPlaceholder(composer);
  const cmsg = `client-${Date.now()}`;
  await cinput.fill(cmsg);
  await cinput.press("Enter");
  await expect(clientPage.getByText(cmsg)).toBeVisible({ timeout: 10000 });

  await ctx.close();
});

test("browser: employee opens a client room; the message routes only there", async ({
  page,
  browser,
}) => {
  const clientName = `Acme-${Date.now()}`;

  // Admin creates a named client link.
  await page.goto(ADMIN_LINK);
  await page.getByRole("button", { name: /Ссылки|Links/ }).click();
  await page.getByPlaceholder(/необязательно|optional/i).fill(clientName);
  await page.getByRole("button", { name: /Создать|Create/ }).first().click();
  const link = await page.locator("code").first().textContent();
  expect(link).toContain("/i/");

  // The client opens its link in a fresh context and waits for the socket.
  const ctx = await browser.newContext();
  const clientPage = await ctx.newPage();
  await clientPage.goto(link!);
  await expect(clientPage.getByPlaceholder(composer)).toBeVisible();
  await expect(clientPage.locator('.beacon[data-state="live"]')).toBeVisible({ timeout: 10000 });

  // Admin returns to chat and opens the client's room from the drawer.
  await page.getByRole("button", { name: /Назад|Back/ }).click();
  await expect(page.locator('.beacon[data-state="live"]')).toBeVisible({ timeout: 10000 });
  await page.getByRole("button", { name: /Чаты|Chats/ }).click();
  await page.getByRole("button", { name: clientName }).click();

  // Admin posts into the client room → the client receives it.
  const msg = `room-${Date.now()}`;
  const input = page.getByPlaceholder(composer);
  await input.fill(msg);
  await input.press("Enter");
  await expect(clientPage.getByText(msg)).toBeVisible({ timeout: 10000 });

  // Isolation: switching to common does not show the client-room message.
  await page.getByRole("button", { name: /Чаты|Chats/ }).click();
  await page.getByRole("button", { name: /командная|team room/ }).click();
  await expect(page.getByText(msg)).toHaveCount(0);

  await ctx.close();
});

test("browser: upload multiple images and they render in the transcript", async ({ page }) => {
  await page.goto(ADMIN_LINK);
  await expect(page.locator('.beacon[data-state="live"]')).toBeVisible({ timeout: 10000 });

  // A tiny but valid 1x1 PNG the server can decode + thumbnail.
  const png = Buffer.from(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
    "base64",
  );
  await page.locator('input[type="file"]').setInputFiles([
    { name: "a.png", mimeType: "image/png", buffer: png },
    { name: "b.png", mimeType: "image/png", buffer: png },
  ]);

  // Both uploads finished → send becomes available; post them in one message.
  const sendBtn = page.getByRole("button", { name: /Отправить|Send/ });
  await expect(sendBtn).toBeVisible({ timeout: 10000 });
  await sendBtn.click();

  // Two images appear in the transcript, served from the attachments endpoint.
  const imgs = page.locator("main img");
  await expect(imgs).toHaveCount(2, { timeout: 10000 });
  await expect(imgs.first()).toHaveAttribute("src", /\/api\/attachments\//);
});

// --- WebRTC call signaling (deterministic: raw WS, no media) ---------------
// Drives the `call-*` protocol over a raw WebSocket inside the page (the auth
// cookie rides along), so we test the server's signaling plane — offer
// generation, room-scoped ringing, and call teardown — without headless media.

async function openWs(page: import("@playwright/test").Page): Promise<void> {
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        const w = window as unknown as Record<string, unknown>;
        w.__frames = [];
        w.__waiters = [];
        const ws = new WebSocket(`ws://${location.host}/ws`);
        w.__ws = ws;
        ws.onmessage = (e) => {
          const f = JSON.parse(e.data);
          (w.__frames as unknown[]).push(f);
          w.__waiters = (w.__waiters as { type: string; resolve: (f: unknown) => void }[]).filter(
            (wt) => {
              if (wt.type === f.type) {
                wt.resolve(f);
                return false;
              }
              return true;
            },
          );
        };
        ws.onopen = () => resolve();
      }),
  );
}

async function waitFrame(
  page: import("@playwright/test").Page,
  type: string,
): Promise<Record<string, unknown>> {
  return (await page.evaluate(
    (type) =>
      new Promise<unknown>((resolve, reject) => {
        const w = window as unknown as Record<string, unknown>;
        const existing = (w.__frames as { type: string }[]).find((f) => f.type === type);
        if (existing) return resolve(existing);
        const to = setTimeout(() => reject(new Error("timeout waiting for " + type)), 8000);
        (w.__waiters as unknown[]).push({
          type,
          resolve: (f: unknown) => {
            clearTimeout(to);
            resolve(f);
          },
        });
      }),
    type,
  )) as Record<string, unknown>;
}

async function sendFrame(
  page: import("@playwright/test").Page,
  frame: Record<string, unknown>,
): Promise<void> {
  await page.evaluate((frame) => {
    (window as unknown as { __ws: WebSocket }).__ws.send(JSON.stringify(frame));
  }, frame);
}

test("browser: call signaling — start offers, rings the room, leave ends it", async ({
  page,
  browser,
}) => {
  // Admin logs in, then mints a second employee and signs them in elsewhere.
  await page.goto(ADMIN_LINK);
  const created = await page.evaluate(async () => {
    const r = await fetch("/api/principals", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ kind: "user" }),
    });
    return r.json();
  });
  expect(created.url).toContain("/i/");

  const ctx = await browser.newContext();
  const emp = await ctx.newPage();
  await emp.goto(created.url);
  await expect(emp.getByPlaceholder(composer)).toBeVisible();

  // Both open a raw WS; employees default to the common room.
  await openWs(page);
  await openWs(emp);

  // Admin starts a call in common → the server answers with an SDP offer…
  await sendFrame(page, { type: "call-start", room_id: "common" });
  const offer = await waitFrame(page, "call-offer");
  expect(typeof offer.sdp).toBe("string");
  expect(offer.sdp as string).toContain("m=audio"); // a real Opus offer
  const callId = offer.call_id as string;
  expect(callId).toBeTruthy();

  // …and rings the rest of the room (the other employee).
  const ring = await waitFrame(emp, "call-ringing");
  expect(ring.room_id).toBe("common");
  expect(ring.call_id).toBe(callId);
  expect(ring.from).toBeTruthy();

  // Admin (the only participant) leaves → the call ends for the room.
  await sendFrame(page, { type: "call-leave", call_id: callId });
  const ended = await waitFrame(emp, "call-ended");
  expect(ended.call_id).toBe(callId);

  await ctx.close();
});

test("browser: anonymous message notifies employees (toast + unread badge)", async ({
  page,
  browser,
}) => {
  await page.goto(ADMIN_LINK);
  await expect(page.locator('.beacon[data-state="live"]')).toBeVisible({ timeout: 10000 });

  // Mint an anonymous client and open their chat in a separate context.
  const client = await page.evaluate(async () => {
    const r = await fetch("/api/principals", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ kind: "client" }),
    });
    return r.json();
  });
  expect(client.url).toContain("/i/");

  const ctx = await browser.newContext();
  const cp = await ctx.newPage();
  await cp.goto(client.url);
  await expect(cp.locator('.beacon[data-state="live"]')).toBeVisible({ timeout: 10000 });

  // Admin stays in the common room; the client writes in their own room.
  const msg = `ping-${Date.now()}`;
  const cinput = cp.getByPlaceholder(composer);
  await cinput.fill(msg);
  await cinput.press("Enter");

  // Admin (cross-room) gets a toast previewing the message + an unread badge.
  await expect(page.getByText(msg)).toBeVisible({ timeout: 10000 });
  await expect(page.getByRole("button", { name: /Чаты|Chats/ })).toContainText("1");

  await ctx.close();
});

test("browser: presence counts online employees in the drawer", async ({ page, browser }) => {
  await page.goto(ADMIN_LINK);
  await expect(page.locator('.beacon[data-state="live"]')).toBeVisible({ timeout: 10000 });

  // Mint a second employee and sign them in elsewhere.
  const created = await page.evaluate(async () => {
    const r = await fetch("/api/principals", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ kind: "user" }),
    });
    return r.json();
  });
  const ctx = await browser.newContext();
  const emp = await ctx.newPage();
  await emp.goto(created.url);
  await expect(emp.locator('.beacon[data-state="live"]')).toBeVisible({ timeout: 10000 });

  // Admin opens the rooms drawer; the common room shows 2 employees online.
  await page.getByRole("button", { name: /Чаты|Chats/ }).click();
  await expect(page.getByRole("button", { name: /командная|team room/ })).toContainText("2");

  await ctx.close();
});

test("api: create an anonymous client (link + room)", async ({ playwright, baseURL }) => {
  const admin = await playwright.request.newContext({ baseURL });
  await admin.get(ADMIN_LINK);

  const res = await admin.post("/api/principals", { data: { kind: "client" } });
  expect(res.ok()).toBeTruthy();
  const created = await res.json();
  expect(created.principal_id).toBeTruthy();
  expect(created.url).toMatch(/^\/i\//);

  // The anonymous client signs in via the link…
  const client = await playwright.request.newContext({ baseURL });
  await client.get(created.url);
  const me = await (await client.get("/api/me")).json();
  expect(me.kind).toBe("client");
  expect(me.is_admin).toBe(false);

  // …and is given exactly one room (its own), auto-created on principal creation.
  const rooms = await (await client.get("/api/rooms")).json();
  expect(Array.isArray(rooms)).toBeTruthy();
  expect(rooms.length).toBe(1);
  expect(rooms[0].kind).toBe("client");

  await admin.dispose();
  await client.dispose();
});

test("api: auth gate, admin vs client, revoke, logout", async ({ playwright, baseURL }) => {
  const admin = await playwright.request.newContext({ baseURL });
  await admin.get(ADMIN_LINK); // sets the session cookie in this context

  const me = await (await admin.get("/api/me")).json();
  expect(me.kind).toBe("user");
  expect(me.is_admin).toBe(true);

  // Admin creates a client link.
  const created = await (
    await admin.post("/api/principals", { data: { kind: "client" } })
  ).json();
  expect(created.url).toContain("/i/");

  // Client signs in via that link in its own context.
  const client = await playwright.request.newContext({ baseURL });
  await client.get(created.url);
  const cme = await (await client.get("/api/me")).json();
  expect(cme.kind).toBe("client");
  expect(cme.is_admin).toBe(false);

  // Client is blocked from admin endpoints.
  expect((await client.get("/api/principals")).status()).toBe(403);

  // Revoke the client link → it no longer logs anyone in.
  await admin.post(`/api/principals/${created.principal_id}/revoke`);
  const reuse = await playwright.request.newContext({ baseURL });
  await reuse.get(created.url);
  expect(await (await reuse.get("/api/me")).json()).toBeNull();

  // Logout invalidates the admin session.
  expect((await admin.post("/api/auth/logout")).status()).toBe(200);
  expect(await (await admin.get("/api/me")).json()).toBeNull();

  await admin.dispose();
  await client.dispose();
  await reuse.dispose();
});

test("api: integration token creates a client and posts the order", async ({
  playwright,
  baseURL,
}) => {
  const admin = await playwright.request.newContext({ baseURL });
  await admin.get(ADMIN_LINK);

  // Admin mints an API token for an integration.
  const created = await (
    await admin.post("/api/integrations", { data: { name: "CRM" } })
  ).json();
  expect(created.token).toMatch(/^zk_/);

  // The integration authenticates with Bearer (no cookies).
  const api = await playwright.request.newContext({
    baseURL,
    extraHTTPHeaders: { Authorization: `Bearer ${created.token}` },
  });

  const who = await (await api.get("/api/v1/me")).json();
  expect(who.kind).toBe("bot");

  // It creates a client + room + link, seeding the order as the first message.
  const order = `order-${Date.now()}`;
  const res = await api.post("/api/v1/clients", {
    data: { name: "Acme", order },
  });
  expect(res.ok()).toBeTruthy();
  const lead = await res.json();
  expect(lead.url).toMatch(/^\/i\//);
  expect(lead.room_id).toBeTruthy();

  // The seeded order lands in the room's history (writes are batched, so poll).
  await expect
    .poll(async () => {
      const msgs = await (await api.get(`/api/v1/rooms/${lead.room_id}/messages`)).json();
      return msgs.some((m: { body: string }) => m.body === order);
    }, { timeout: 5000 })
    .toBeTruthy();

  // It can post into the room by client id, as the bot.
  const reply = `bot-reply-${Date.now()}`;
  const posted = await api.post(`/api/v1/clients/${lead.client_id}/messages`, {
    data: { body: reply },
  });
  expect(posted.ok()).toBeTruthy();
  expect((await posted.json()).author_name).toBe("CRM");

  // No Bearer → 401.
  const anon = await playwright.request.newContext({ baseURL });
  expect((await anon.get("/api/v1/me")).status()).toBe(401);

  await admin.dispose();
  await api.dispose();
  await anon.dispose();
});
