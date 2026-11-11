import { test, expect } from '@playwright/test';
test('dashboard scaffold renders', async ({ page }) => {
  await page.goto('/');
  await expect(page).toHaveTitle('Arxcovo');
  await expect(page.getByRole('heading', { name: 'Arxcovo' })).toBeVisible();
  await expect(page.getByText('Workspace scaffold.', { exact: false })).toBeVisible();
});
