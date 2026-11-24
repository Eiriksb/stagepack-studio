---
theme: default
title: "A made-up talk"
canvasWidth: 1920
aspectRatio: 16/9
layout: default
clicks: 5
---

<style>
:global(:root) {
  --slidev-theme-primary: #2563eb;
}
:global(.slidev-layout) {
  background: #ffffff;
  color: #1e293b;
  font-family: "Inter", sans-serif;
  font-size: 40px;
}
:global(.slidev-layout h1) {
  font-size: 64px;
}
:global(.slidev-layout.cover h1), :global(.slidev-layout.section h1) {
  font-size: 96px;
}
:global(.slidev-layout code), :global(.slidev-layout pre) {
  font-family: "JetBrains Mono", monospace;
}
</style>

# Steps

<div>
<img style="position:absolute;left:100px;top:300px;width:800px;height:400px" v-if="$clicks === 0" src="/baked-1-0-0.png" alt="">
<img style="position:absolute;left:100px;top:300px;width:800px;height:400px" v-if="$clicks === 1" src="/baked-1-0-1.png" alt="">
<img style="position:absolute;left:100px;top:300px;width:800px;height:400px" v-if="$clicks === 2" src="/baked-1-0-2.png" alt="">
<img style="position:absolute;left:100px;top:300px;width:800px;height:400px" v-if="$clicks >= 3" src="/baked-1-0-3.png" alt="">
</div>

<img style="position:absolute;left:1000px;top:300px;width:400px;height:400px" src="/baked-1-1.png" alt="">

<div style="transition:none" v-click="2">
<img style="position:absolute;left:100px;top:750px;width:400px;height:300px" v-if="$clicks < 2" src="/baked-1-2-0.png" alt="">
<img style="position:absolute;left:100px;top:750px;width:400px;height:300px" v-if="$clicks >= 2 && $clicks < 5" src="/baked-1-2-2.png" alt="">
<img style="position:absolute;left:100px;top:750px;width:400px;height:300px" v-if="$clicks >= 5" src="/baked-1-2-5.png" alt="">
</div>
