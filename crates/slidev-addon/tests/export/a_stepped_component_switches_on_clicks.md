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
layout: default
transition: none
clicks: 5
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

# Steps

<div>
<img style="position:absolute;left:100px;top:300px;width:800px;height:400px" v-if="$clicks === 0" src="/stagepack-baked/1-0-0.png" alt="">
<img style="position:absolute;left:100px;top:300px;width:800px;height:400px" v-if="$clicks === 1" src="/stagepack-baked/1-0-1.png" alt="">
<img style="position:absolute;left:100px;top:300px;width:800px;height:400px" v-if="$clicks === 2" src="/stagepack-baked/1-0-2.png" alt="">
<img style="position:absolute;left:100px;top:300px;width:800px;height:400px" v-if="$clicks >= 3" src="/stagepack-baked/1-0-3.png" alt="">
</div>

<img style="position:absolute;left:1000px;top:300px;width:400px;height:400px" src="/stagepack-baked/1-1.png" alt="">

<div style="transition:none" v-click="2">
<img style="position:absolute;left:100px;top:750px;width:400px;height:300px" v-if="$clicks < 2" src="/stagepack-baked/1-2-0.png" alt="">
<img style="position:absolute;left:100px;top:750px;width:400px;height:300px" v-if="$clicks >= 2 && $clicks < 5" src="/stagepack-baked/1-2-2.png" alt="">
<img style="position:absolute;left:100px;top:750px;width:400px;height:300px" v-if="$clicks >= 5" src="/stagepack-baked/1-2-5.png" alt="">
</div>
