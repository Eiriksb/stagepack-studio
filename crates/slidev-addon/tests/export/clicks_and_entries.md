---
theme: default
title: "A made-up talk"
canvasWidth: 1920
aspectRatio: 16/9
layout: two-cols
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

# Clicks

<v-clicks>

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
