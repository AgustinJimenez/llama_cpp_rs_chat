import { test, expect } from '@playwright/test';

test.describe('Sidebar Functionality', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');
  });

  test('sidebar can be toggled open and closed', async ({ page }) => {
    // Check initial state - sidebar should be visible but closed by default
    const sidebar = page.getByTestId('sidebar');
    await expect(sidebar).toBeVisible();
    
    // Find the toggle button (hamburger menu)
    const toggleButton = page.getByTestId('sidebar-toggle');
    await expect(toggleButton).toBeVisible();
    
    // Initial state should show collapsed buttons
    const collapsedNewChat = page.getByTestId('collapsed-new-chat');
    await expect(collapsedNewChat).toBeVisible();
    
    // Click to expand sidebar
    await toggleButton.click();
    
    // Check if sidebar is expanded (shows full buttons with text)
    const expandedNewChat = page.getByTestId('new-chat-btn');
    await expect(expandedNewChat).toBeVisible();
    
    // Click to collapse sidebar again
    await toggleButton.click();
    
    // Should be back to collapsed state
    await expect(collapsedNewChat).toBeVisible();
    
    console.log('✅ Sidebar toggle test completed!');
  });

  test('new chat button creates a new conversation', async ({ page }) => {
    // Send a message first to have something to clear
    const messageInput = page.getByTestId('message-input');
    const sendButton = page.getByTestId('send-button');
    
    await messageInput.fill('Test message for new chat');
    await sendButton.click();
    
    // Wait for the message to appear
    await expect(page.getByTestId('message-user')).toBeVisible();
    
    // Click new chat button (handle both collapsed and expanded states)
    const newChatBtn = page.getByTestId('new-chat-btn');
    const collapsedNewChatBtn = page.getByTestId('collapsed-new-chat');
    
    if (await collapsedNewChatBtn.isVisible()) {
      await collapsedNewChatBtn.click();
    } else if (await newChatBtn.isVisible()) {
      await newChatBtn.click();
    } else {
      throw new Error('No new chat button found');
    }
    
    // Verify messages are cleared
    await expect(page.getByTestId('message-user')).toHaveCount(0);
    await expect(page.getByTestId('message-assistant')).toHaveCount(0);
    
    // Verify input is empty
    await expect(messageInput).toHaveValue('');
    
    console.log('✅ New chat functionality test completed!');
  });

  test('settings button opens settings modal', async ({ page }) => {
    // Find settings button (handle both collapsed and expanded states)
    const settingsBtn = page.getByTestId('settings-btn');
    const collapsedSettingsBtn = page.getByTestId('collapsed-settings');
    
    if (await collapsedSettingsBtn.isVisible()) {
      await collapsedSettingsBtn.click();
    } else if (await settingsBtn.isVisible()) {
      await settingsBtn.click();
    } else {
      throw new Error('No settings button found');
    }
    
    // Verify settings modal opens
    const settingsModal = page.locator('[role="dialog"]');
    await expect(settingsModal).toBeVisible();
    
    // Verify modal content (use more specific selector)
    const modalTitle = page.getByRole('heading', { name: 'Configuration' });
    await expect(modalTitle).toBeVisible();
    
    // Close modal
    const cancelButton = page.getByRole('button', { name: 'Cancel' });
    await cancelButton.click();
    
    // Verify modal is closed
    await expect(settingsModal).not.toBeVisible();
    
    console.log('✅ Settings modal test completed!');
  });

  test('sidebar shows conversation list when expanded', async ({ page }) => {
    // Ensure sidebar is expanded
    const sidebar = page.getByTestId('sidebar');
    await expect(sidebar).toBeVisible();
    
    // Check for conversations section
    const conversationsSection = page.getByText('Conversations');
    
    // The conversations section should be visible when sidebar is expanded
    // If collapsed, we won't see the text
    const isExpanded = await conversationsSection.isVisible();
    
    if (isExpanded) {
      // Sidebar is expanded, we can see the conversations list
      const conversationsList = page.getByTestId('conversations-list');
      await expect(conversationsList).toBeVisible();
      
      console.log('✅ Conversations list visible in expanded sidebar');
    } else {
      // Sidebar might be collapsed, let's expand it
      const toggleButton = page.getByTestId('sidebar-toggle');
      await toggleButton.click();
      
      // Now check for conversations
      await expect(conversationsSection).toBeVisible();
      const conversationsList = page.getByTestId('conversations-list');
      await expect(conversationsList).toBeVisible();
      
      console.log('✅ Sidebar expanded and conversations list visible');
    }
    
    console.log('✅ Sidebar conversation list test completed!');
  });

  test('sidebar is responsive on mobile viewport', async ({ page }) => {
    // Set mobile viewport
    await page.setViewportSize({ width: 375, height: 667 });
    
    // Check that sidebar is still functional on mobile
    const sidebar = page.getByTestId('sidebar');
    await expect(sidebar).toBeVisible();
    
    // Check toggle functionality on mobile
    const toggleButton = page.getByTestId('sidebar-toggle');
    await expect(toggleButton).toBeVisible();
    
    // Test toggle (may behave differently on mobile)
    await toggleButton.click();
    
    // Sidebar should still be responsive
    await expect(sidebar).toBeVisible();
    
    console.log('✅ Mobile sidebar responsiveness test completed!');
  });
});