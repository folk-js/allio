# Folk Interlay

## Getting started

Check out the [Contributing Guide](/CONTRIBUTING.md) to setting up the repo.

## Overview

Interlay is an exploration in how rigid, siloed desktop applications can be reappropriated into a more malleable computing substrate by leveraging existing accessibility and windowing infrastructure. We use accessibility trees as a freely addressable surface for the user interface of applications such that they can be programmatically queried, traversed, and edited. For more background check out our [paper](https://folkjs.org/live-2025/).

## Architecture

![Interlay architecture diagram](/docs/interlay_architecture.png)

There is a websocket server (written in Rust) that interfaces directly with operating systems accessibility and windowing infrastructure and normalizes them by exposing an read/write ARIA-like tree. We use Tarui to create a invisible desktop overlay so a plurality of web views can interact with this data.
