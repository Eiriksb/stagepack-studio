---
theme: default
title: "A made-up talk"
canvasWidth: 1920
aspectRatio: 16/9
colorSchema: light
fonts:
  sans: "Inter"
  mono: "JetBrains Mono"
  provider: none
layout: two-cols
transition: none
clicks: 4
---

<style>
:global(:root) {
  --slidev-theme-primary: #2563eb;
  --slidev-code-font-family: "JetBrains Mono", monospace;
  --slidev-code-font-size: 32px;
  --slidev-code-line-height: 1.25;
}
:global(.slidev-page) {
  height: 100%;
  background: #ffffff;
  color: #1e293b;
  font-family: "Inter", sans-serif;
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-layout) {
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-page p), :global(.slidev-page li) {
  line-height: 1.25;
}
:global(.slidev-page code) {
  font-family: "JetBrains Mono", monospace;
}
:global(.slidev-page h1) {
  font-size: 64px;
  line-height: 1.25;
}
:global(.slidev-page h2) {
  font-size: 48px;
  line-height: 1.25;
}
:global(.slidev-page h3) {
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-page h4) {
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-page h5) {
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-page h6) {
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-layout.cover h1), :global(.slidev-layout.section h1) {
  font-size: 96px;
  line-height: 1.25;
}
:global(.slidev-page h6), :global(.slidev-layout h6:not(.opacity-100)) {
  font-weight: bold;
  text-transform: none;
  letter-spacing: normal;
  opacity: 1;
}
</style>

# Clicks

<v-clicks at="1">

- One
- Two
- Three

</v-clicks>

::right::

<div v-motion :initial="{y:40,opacity:0}" :click-2="{y:0,opacity:1}">

On the second click.

</div>

<div style="position:absolute;left:100px;top:800px;width:300px;height:100px;box-sizing:border-box" v-click="1">

Fades in

</div>

<div style="position:absolute;left:500px;top:800px;width:300px;height:100px;box-sizing:border-box;transition:none" v-click="3">

Appears

</div>

<div style="position:absolute;left:900px;top:800px;width:300px;height:100px;box-sizing:border-box" v-motion :initial="{y:40,opacity:0}" :click-4="{y:0,opacity:1}">

Flies up

</div>

<div style="position:absolute;left:1300px;top:800px;width:300px;height:100px;box-sizing:border-box">

Always there

</div>

---
layout: default
transition: none
clicks: 2
---

<style>
:global(:root) {
  --slidev-theme-primary: #2563eb;
  --slidev-code-font-family: "JetBrains Mono", monospace;
  --slidev-code-font-size: 32px;
  --slidev-code-line-height: 1.25;
}
:global(.slidev-page) {
  height: 100%;
  background: #ffffff;
  color: #1e293b;
  font-family: "Inter", sans-serif;
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-layout) {
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-page p), :global(.slidev-page li) {
  line-height: 1.25;
}
:global(.slidev-page code) {
  font-family: "JetBrains Mono", monospace;
}
:global(.slidev-page h1) {
  font-size: 64px;
  line-height: 1.25;
}
:global(.slidev-page h2) {
  font-size: 48px;
  line-height: 1.25;
}
:global(.slidev-page h3) {
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-page h4) {
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-page h5) {
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-page h6) {
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-layout.cover h1), :global(.slidev-layout.section h1) {
  font-size: 96px;
  line-height: 1.25;
}
:global(.slidev-page h6), :global(.slidev-layout h6:not(.opacity-100)) {
  font-weight: bold;
  text-transform: none;
  letter-spacing: normal;
  opacity: 1;
}
</style>

# Only a fly-up

<div style="position:absolute;left:100px;top:300px;width:300px;height:100px;box-sizing:border-box" v-motion :initial="{y:40,opacity:0}" :click-2="{y:0,opacity:1}">

Flies up

</div>

---
layout: default
transition: none
---

<style>
:global(:root) {
  --slidev-theme-primary: #2563eb;
  --slidev-code-font-family: "JetBrains Mono", monospace;
  --slidev-code-font-size: 32px;
  --slidev-code-line-height: 1.25;
}
:global(.slidev-page) {
  height: 100%;
  background: #ffffff;
  color: #1e293b;
  font-family: "Inter", sans-serif;
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-layout) {
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-page p), :global(.slidev-page li) {
  line-height: 1.25;
}
:global(.slidev-page code) {
  font-family: "JetBrains Mono", monospace;
}
:global(.slidev-page h1) {
  font-size: 64px;
  line-height: 1.25;
}
:global(.slidev-page h2) {
  font-size: 48px;
  line-height: 1.25;
}
:global(.slidev-page h3) {
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-page h4) {
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-page h5) {
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-page h6) {
  font-size: 40px;
  line-height: 1.25;
}
:global(.slidev-layout.cover h1), :global(.slidev-layout.section h1) {
  font-size: 96px;
  line-height: 1.25;
}
:global(.slidev-page h6), :global(.slidev-layout h6:not(.opacity-100)) {
  font-weight: bold;
  text-transform: none;
  letter-spacing: normal;
  opacity: 1;
}
</style>

# No clicks
