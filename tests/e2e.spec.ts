// Zenithar end-to-end tests (Playwright). Run via `make e2e`, which starts an
// ephemeral server on a temp DB and passes BASE + ADMIN_LINK.
//
// The browser specs drive the real UI (so they catch issues a fetch-level test
// misses, e.g. assets failing to load under /i/:token). The API spec covers the
// auth gate, revocation, and logout via Playwright request contexts.
import { test, expect } from "@playwright/test";

const ADMIN_LINK = process.env.ADMIN_LINK!;
const composer = /команде|team/i; // message placeholder (RU default / EN)

test.describe.configure({ mode: "serial" });

test("browser: link login renders the chat and sends a message", async ({ page }) => {
  await page.goto(ADMIN_LINK);

  // The token is dropped from the URL after the cookie is set.
  await expect(page).toHaveURL(/\/$/);
  // App actually mounted (would be blank if assets failed to load).
  await expect(page.getByText("Zenithar")).toBeVisible();

  const input = page.getByPlaceholder(composer);
  await expect(input).toBeVisible();

  // Wait for the WebSocket to be live before sending (send is a no-op otherwise).
  await expect(page.locator('.beacon[data-state="live"]')).toBeVisible({ timeout: 10000 });

  const msg = `pw-${Date.now()}`;
  await input.fill(msg);
  await input.press("Enter");
  await expect(page.getByText(msg)).toBeVisible();
});

test("browser: admin self-rename sticks", async ({ page }) => {
  await page.goto(ADMIN_LINK);
  await page.getByRole("button", { name: "admin" }).click();
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
  await page.getByRole("button", { name: /Создать|Create/ }).click();

  const link = await page.locator("code").first().textContent();
  expect(link).toContain("/i/");

  // Open the link in a fresh context (separate cookie jar) = the client.
  const ctx = await browser.newContext();
  const clientPage = await ctx.newPage();
  await clientPage.goto(link!);
  await expect(clientPage.getByPlaceholder(composer)).toBeVisible();
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
  await page.getByRole("button", { name: /Создать|Create/ }).click();
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
