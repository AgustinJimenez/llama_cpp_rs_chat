import AxeBuilder from '@axe-core/playwright';
import { test, expect } from '@playwright/test';

// Exclude Vite's dev-tool overlay which injects its own shadow DOM with known
// contrast/landmark issues unrelated to our app.
const axeBuilder = (page: Parameters<typeof AxeBuilder>[0]['page']) =>
  new AxeBuilder({ page }).exclude('vite-plugin-checker-error-overlay');

test.describe('Accessibility', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    // Use 'load' (not 'networkidle') so CI passes without a live backend.
    // The app polls /api/* continuously; networkidle never fires when those
    // connections are refused.
    await page.waitForLoadState('load');
  });

  test('main page', async ({ page }) => {
    const results = await axeBuilder(page).analyze();
    expect(results.violations).toEqual([]);
  });

  test('agent picker dropdown', async ({ page }) => {
    // The header agent picker opens an inline dropdown (not a dialog)
    await page.getByTitle('Select agent').click();
    await page.waitForSelector('text=No agent', { state: 'visible' });

    const results = await axeBuilder(page).analyze();
    expect(results.violations).toEqual([]);
  });

  test('app settings modal', async ({ page }) => {
    await page.getByRole('button', { name: /app settings/i }).click();
    await page.getByRole('dialog').waitFor({ state: 'visible' });

    const results = await axeBuilder(page).analyze();
    expect(results.violations).toEqual([]);
  });

  test('system monitor sidebar', async ({ page }) => {
    await page.getByTitle('Toggle system monitor').click();
    await page.waitForTimeout(300);

    const results = await axeBuilder(page).analyze();
    expect(results.violations).toEqual([]);
  });

  // Requires a live backend with conversations in the database — skip in CI.
  test('conversation context menu', async ({ page }) => {
    test.skip(!!process.env.CI, 'Requires a live backend with conversation history');
    const firstConversation = page.locator('[data-testid="conversation-0"]');
    await firstConversation.hover();
    await page.getByRole('button', { name: 'Conversation options' }).first().click();
    await page.waitForTimeout(200);

    const results = await axeBuilder(page).analyze();
    expect(results.violations).toEqual([]);
  });

  test('mobile sidebar overlay', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 812 });
    // force:true bypasses the off-screen nav that overlaps the hamburger on small viewports
    await page.getByTestId('chat-header').getByRole('button', { name: 'Toggle sidebar' }).click({ force: true });
    await page.waitForTimeout(200);

    const results = await axeBuilder(page).analyze();
    expect(results.violations).toEqual([]);
  });
});
