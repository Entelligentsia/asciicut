# Asciicut UI Integration Guide — Incorporating Nibbles

This document details the recommended code and CSS updates for incorporating **Nibbles the Beaver** into the Asciicut SolidJS frontend (`web/`).

---

## 1. Favicon Integration (`web/index.html`)

To update the browser tab favicon, link the transparent logomark in `web/index.html`:

```html
<!-- Line 6 onwards in web/index.html -->
<title>asciicut — the cutting room</title>
<link rel="icon" type="image/png" href="/assets/logomark_head_transparent.png" />
```

---

## 2. Navigation Header Logo (`web/src/views/Editor.tsx` / `Welcome.tsx`)

Add a tiny Nibbles head logomark next to the title text in header navigation areas.

```tsx
<div class="flex items-center gap-2">
  <img 
    src="/assets/logomark_head_transparent.png" 
    class="h-6 w-6 object-contain" 
    alt="Asciicut Logo" 
  />
  <span class="font-mono font-bold text-lg text-slate-100">asciicut</span>
</div>
```

---

## 3. Welcome View Mascot Hero (`web/src/views/Welcome.tsx`)

Show the large mascot above the primary title. We can replace the simple styling/spacers at the top of the welcome card with the transparent master logo, styled with a soft floating animation.

```tsx
// Insert above the title/lede in web/src/views/Welcome.tsx
<div class="wel-mascot-wrap">
  <img 
    src="/assets/logo_master_transparent.png" 
    class="wel-mascot animate-float" 
    alt="Nibbles the Beaver" 
  />
</div>
```

---

## 4. Loader & WASM Init Overlay (`web/src/index.tsx`)

Render the progress/hamster wheel illustration when loading the WASM backend or executing a long-running composition export.

```tsx
<div class="fixed inset-0 bg-slate-950/80 backdrop-blur-sm flex flex-col items-center justify-center z-50">
  <img 
    src="/assets/ui_loading_transparent.png" 
    class="w-40 h-40 object-contain mb-4" 
    alt="Loading Engine" 
  />
  <div class="w-48 bg-slate-800 rounded-full h-1.5 overflow-hidden">
    <div class="bg-amber-500 h-full w-3/4 rounded-full animate-pulse"></div>
  </div>
  <span class="text-xs font-mono text-slate-400 mt-2">initializing core engine...</span>
</div>
```

---

## 5. UI Empty States (`web/src/views/Welcome.tsx`)

For panels that display "empty" data lists (e.g., when the `Recent` list length is 0), replace the empty space with a clean illustration of Nibbles tangled in tape:

```tsx
<div class="flex flex-col items-center justify-center py-8 opacity-60">
  <img 
    src="/assets/ui_empty_state_transparent.png" 
    class="w-24 h-24 object-contain mb-2" 
    alt="No recordings" 
  />
  <span class="text-xs font-mono text-slate-400">no recent recordings loaded</span>
</div>
```

---

## 6. CSS Animations & Styling (`web/src/styles.css`)

To make the mascot look responsive and "alive" (as per the application's rich design guidelines), append the following rules to the bottom of the welcome styling section:

```css
/* Custom Mascot Styles & Floating Animation */
.wel-mascot-wrap {
  display: flex;
  justify-content: center;
  margin-bottom: 1.5rem;
}

.wel-mascot {
  width: 192px;
  height: 192px;
  object-fit: contain;
}

/* Slow, friendly floating animation */
.animate-float {
  animation: float 4s ease-in-out infinite;
}

@keyframes float {
  0%, 100% {
    transform: translateY(0);
  }
  50% {
    transform: translateY(-8px);
  }
}
```
