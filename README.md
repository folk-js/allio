# Accessibility (AX) In/Out

An experimental system to expose accessibility trees as read-write interfaces for all running applications.

This lets you augment existing interfaces, link apps together, use window geometry, and write js/ts to render new UI in a transparent webview.

For more background check out our [paper](https://folkjs.org/live-2025/).

![AXIO architecture diagram](/docs/axio_architecture.png)

Check out the [Contributing Guide](/CONTRIBUTING.md) to setting up the repo.

## Goals/Challenges

Some technical challenges of this project are:

- Efficient collection of window geometry
- Assosiating windows with their accessibility trees (as these are semi-sandboxed)
- Efficiently getting accessibility trees
- Making the trees 'reactive' and live
- Handling tree and element lifecycle
- Syncing between Rust and Web
- ...many more
