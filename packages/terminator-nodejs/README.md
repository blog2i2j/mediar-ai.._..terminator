# @mediar-ai/terminator

Node.js/TypeScript bindings for the Terminator Rust library - AI-native GUI automation for Windows, macOS, and Linux.

## Installation

```bash
npm install @mediar-ai/terminator
# or
bun install @mediar-ai/terminator
# or
yarn add @mediar-ai/terminator
```

## Quick Start

```javascript
const { Desktop } = require('@mediar-ai/terminator');

async function main() {
  const desktop = new Desktop();
  
  // Get root element
  const root = desktop.root();
  console.log('Root element:', root.role(), root.name());
  
  // Find and click a button
  const locator = desktop.locator('role:button');
  try {
    const button = await locator.first(0); // timeout in ms (0 = immediate, no retry)
    console.log('Found button:', button.name());
    await button.click();
  } catch (error) {
    console.log('Button not found:', error.message);
  }
  
  // Take a screenshot
  const screenshot = await desktop.captureScreen();
  console.log(`Screenshot: ${screenshot.width}x${screenshot.height}`);
  
  // Run a command
  const result = await desktop.runCommand('echo hello', 'echo hello');
  console.log('Command output:', result.stdout);
}

main().catch(console.error);
```

## TypeScript Support

This package includes TypeScript definitions out of the box:

```typescript
import { Desktop, ElementNotFoundError } from '@mediar-ai/terminator';

const desktop = new Desktop();
const root = desktop.root();
```

## Error Handling

The library provides specific error types for better error handling:

```javascript
const { 
  Desktop, 
  ElementNotFoundError, 
  TimeoutError, 
  PermissionDeniedError 
} = require('@mediar-ai/terminator');

try {
  const button = await desktop.locator('role:button').first(1000); // wait up to 1 second
  await button.click();
} catch (error) {
  if (error instanceof ElementNotFoundError) {
    console.log('Element not found');
  } else if (error instanceof TimeoutError) {
    console.log('Operation timed out');
  } else if (error instanceof PermissionDeniedError) {
    console.log('Permission denied');
  }
}
```

## Platform Support

- ✅ Windows (x64)

## API Reference

### Desktop

- `new Desktop()` - Create a new desktop automation instance
- `root()` - Get the root element
- `applications()` - List all applications
- `locator(selector)` - Create a locator for finding elements
- `captureScreen()` - Take a screenshot
- `runCommand(windowsCmd, unixCmd)` - Run a system command
- `executeBrowserScript(script)` - Execute JavaScript in focused browser tab
- `navigateBrowser(url, browser?)` - Navigate to URL in specified browser
- `delay(delayMs)` - Delay execution for specified milliseconds

### Element

- `click()` - Click the element
- `type(text)` - Type text into the element
- `scrollIntoView()` - Scroll element into view if off-screen
- `role()` - Get element role
- `name()` - Get element name
- `children()` - Get child elements
- `isToggled()` - Check if toggleable control is toggled
- `setToggled(state)` - Set toggle state (checkbox, toggle switch)
- `isSelected()` - Check if element is selected (list item, tree node, tab)
- `setSelected(state)` - Set selection state
- `getRangeValue()` - Get value from range control (slider, progress bar)
- `setRangeValue(value)` - Set value of range control
- `getValue()` - Get value attribute (text inputs, combo boxes)

### Locator

- `first(timeoutMs)` - Get the first matching element (timeout in milliseconds required)
- `all(timeoutMs, depth?)` - Get all matching elements (timeout in milliseconds required)
- `validate(timeoutMs)` - Validate element existence without throwing (returns `{exists, element?, error?}`)
- `waitFor(condition, timeoutMs)` - Wait for element to meet condition ('exists', 'visible', 'enabled', 'focused')
- `timeout(timeoutMs)` - Set default timeout for this locator
- `within(element)` - Scope search to an element
- `locator(selector)` - Chain another selector

## Examples

### Element Validation

The `validate()` method is useful for conditional logic when you need to check if an element exists without throwing an error:

```javascript
const { Desktop } = require('@mediar-ai/terminator');

async function main() {
  const desktop = new Desktop();

  // Check if a submit button exists
  const result = await desktop.locator('role:button|Submit').validate(5000);

  if (result.exists) {
    console.log('Submit button found!');
    await result.element.click();
  } else {
    console.log('Submit button not found, showing alternative flow');
    // Handle the case where element doesn't exist
  }

  // You can also check for errors (invalid selector, platform errors)
  if (result.error) {
    console.error('Validation error:', result.error);
  }
}

main().catch(console.error);
```

**Key differences between `validate()` and `first()`:**
- `first()` throws an error if element is not found
- `validate()` returns `{exists: false}` if element is not found
- Use `validate()` for optional elements or conditional logic
- Use `first()` when you expect the element to exist

### Conditional Waiting

The `waitFor()` method waits for an element to meet a specific condition. This is useful for dynamic UIs where elements change state:

```javascript
const { Desktop } = require('@mediar-ai/terminator');

async function main() {
  const desktop = new Desktop();

  // Wait for a button to become visible (not just exist)
  const submitButton = await desktop
    .locator('role:button|Submit')
    .waitFor('visible', 10000);

  await submitButton.click();

  // Wait for a button to become enabled after form validation
  const nextButton = await desktop
    .locator('role:button|Next')
    .waitFor('enabled', 5000);

  await nextButton.click();

  // Wait for an input field to receive focus
  const nameInput = await desktop
    .locator('role:textfield|Name')
    .waitFor('focused', 3000);
}

main().catch(console.error);
```

**Available conditions:**
- `'exists'` - Wait for element to appear in the UI
- `'visible'` - Wait for element to be visible
- `'enabled'` - Wait for element to be enabled (not disabled)
- `'focused'` - Wait for element to have keyboard focus

**Key differences:**
- `first()` - Waits for element to exist, throws if not found
- `validate()` - Checks if element exists, never throws
- `waitFor()` - Waits for specific condition to be met, throws on timeout

See the [examples directory](https://github.com/mediar-ai/terminator/tree/main/examples) for more usage examples.

## Repository

https://github.com/mediar-ai/terminator 