<a id="readme-top"></a>

[![Contributors][contributors-shield]][contributors-url]
[![Forks][forks-shield]][forks-url]
[![Stargazers][stars-shield]][stars-url]
[![Issues][issues-shield]][issues-url]

<!-- PROJECT LOGO -->
<br />
<div align="center">
  <a href="https://github.com/MatejMa2ur/tikontrol">
    <img src="src/ui/public/favicon.svg" alt="Logo" width="80" height="80">
  </a>

<h3 align="center">tikontrol</h3>

  <p align="center">
    An independent, UniFi-inspired management UI for MikroTik RouterOS
    <br />
    <a href="docs/PLAN.md"><strong>Explore the docs »</strong></a>
    <br />
    <br />
    <a href="https://github.com/MatejMa2ur/tikontrol/issues/new?labels=bug">Report Bug</a>
    &middot;
    <a href="https://github.com/MatejMa2ur/tikontrol/issues/new?labels=enhancement">Request Feature</a>
  </p>
</div>

> **Install on your router in one paste:** see [docs/INSTALL.md](docs/INSTALL.md).

<!-- TABLE OF CONTENTS -->
<details>
  <summary>Table of Contents</summary>
  <ol>
    <li>
      <a href="#about-the-project">About The Project</a>
      <ul>
        <li><a href="#screenshots">Screenshots</a></li>
        <li><a href="#built-with">Built With</a></li>
      </ul>
    </li>
    <li>
      <a href="#getting-started">Getting Started</a>
      <ul>
        <li><a href="#prerequisites">Prerequisites</a></li>
        <li><a href="#installation">Installation</a></li>
      </ul>
    </li>
    <li><a href="#usage">Usage</a></li>
    <li><a href="#project-structure">Project Structure</a></li>
    <li><a href="#hardware">Hardware</a></li>
    <li><a href="#roadmap">Roadmap</a></li>
    <li><a href="#contributing">Contributing</a></li>
    <li><a href="#license">License</a></li>
    <li><a href="#trademarks">Trademarks</a></li>
    <li><a href="#contact">Contact</a></li>
    <li><a href="#acknowledgments">Acknowledgments</a></li>
  </ol>
</details>

<!-- ABOUT THE PROJECT -->
## About The Project

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/dashboard-dark.png">
  <img src="docs/images/dashboard.png" alt="tikontrol dashboard">
</picture>

tikontrol is a clean, opinionated web UI for MikroTik routers, inspired by how UniFi Network organises a home network: dashboard, client list, networks, Wi-Fi, firewall and port forwarding, VPN, topology and alerts.

- Hosted in a RouterOS v7 container. No extra server, no cloud.
- Talks to RouterOS through its own API. RouterOS stays the source of truth, and WinBox/WebFig keep working next to it.
- It can also manage other MikroTik devices on the LAN (APs, switches) from the same place.

**Non-goals**

- Replacing WinBox for advanced config. Raw access stays one click away.
- Features that RouterOS can't back, such as real DPI or a native mobile app. See [RESEARCH.md](docs/RESEARCH.md).

> [!NOTE]
> tikontrol is in the **UI mockup phase**. The pages run on a router snapshot with simulated live numbers, and nothing is written to a router yet.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

### Screenshots

tikontrol has a light and a dark theme. These screenshots use fictional sample data, so no real network appears here. To refresh them, run the mockup, add `?theme=light` or `?theme=dark` to the URL and capture each page.

| Light | Dark |
| --- | --- |
| ![Dashboard, light theme](docs/images/dashboard.png) | ![Dashboard, dark theme](docs/images/dashboard-dark.png) |
| ![Clients, light theme](docs/images/clients.png) | ![Clients, dark theme](docs/images/clients-dark.png) |
| ![Topology, light theme](docs/images/topology.png) | ![Topology, dark theme](docs/images/topology-dark.png) |
| ![Settings, light theme](docs/images/settings.png) | ![Settings, dark theme](docs/images/settings-dark.png) |

The hAP be³ Media preview is simulated from MikroTik's spec sheet:

![Simulated hAP be³ Media dashboard](docs/images/dashboard-be3.png)

### Built With

[![Svelte][Svelte]][Svelte-url]
[![TypeScript][TypeScript]][TypeScript-url]
[![Vite][Vite]][Vite-url]

The planned backend is a single Rust binary with the UI embedded. See [PLAN.md §6](docs/PLAN.md#6-stack).

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- GETTING STARTED -->
## Getting Started

### Prerequisites

- Node.js 24 or newer and npm
- Optional: Python 3 and SSH access to a RouterOS v7 router, to use your own data instead of the sample

### Installation

1. Clone the repo
   ```sh
   git clone https://github.com/MatejMa2ur/tikontrol.git
   cd tikontrol
   ```
2. Optional: take a read-only snapshot of your router over SSH
   ```sh
   src/scripts/snapshot.py > src/ui/src/lib/snapshot.json
   ```
3. Install the dependencies and start the mockup
   ```sh
   cd src/ui
   npm install
   npm run dev
   ```

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- USAGE -->
## Usage

Open <http://localhost:5173>. You don't need to be on the router's network: without a snapshot, the UI falls back to the fictional `src/ui/src/lib/sample.json`. Live numbers are simulated around the snapshot.

The make, model, CPU, memory and Wi-Fi capabilities are detected from what the router reports, so there is nothing to set up per model. To preview a router you don't own, open <http://localhost:5173/?device=be3>. It simulates a hAP be³ Media from MikroTik's spec sheet, with the sample network's clients and traffic.

To run the tests from the repo root:

```sh
node --test tests/hardware.test.ts
```

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- PROJECT STRUCTURE -->
## Project Structure

```
docs/          Plan, research, feasibility study, feature tracker and screenshots
src/scripts/   Router snapshot, anonymiser and progress counter
src/ui/        Svelte 5 + Vite mockup
tests/         Hardware detection tests
```

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- HARDWARE -->
## Hardware

| | hAP ac² (current) | hAP be³ Media (planned) |
| --- | --- | --- |
| Architecture | ARM 32-bit | ARM 64-bit |
| RAM | 128 MiB | 2 GB |
| Storage | 16 MiB flash | 512 MB NAND + microSD |
| Wi-Fi | Legacy `wireless` package | WiFi 7, new `wifi` package |
| tikontrol | Too tight for a full on-router controller | Planned on-router host |

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- ROADMAP -->
## Roadmap

**Progress is tracked in [FEATURES.md](docs/FEATURES.md)**: every UniFi page and feature, what backs it on MikroTik, and its status. See [PLAN.md](docs/PLAN.md) for the plan and [RESEARCH.md](docs/RESEARCH.md) for prior art and platform limits. [FEASIBILITY.md](docs/FEASIBILITY.md) is the latest study of what is possible, with its source notes in [docs/research](docs/research).

- [ ] Phase 0: Dev environment, connect to the router
- [ ] Phase 1: Read-only dashboard and clients
- [ ] Phase 2: Safe writes
- [ ] Phase 3: Structural changes through Safe apply
- [ ] Phase 4: Fleet management and extras

**Open questions**

- The hAP ac² is too tight for a full on-router controller. Host it on a separate home server first? (see [RESEARCH.md](docs/RESEARCH.md) §5)
- Fork or contribute to [nasnet-panel](https://github.com/nasnet-community/nasnet-panel) rather than start from zero?

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- CONTRIBUTING -->
## Contributing

Bug reports and ideas are welcome through [issues](https://github.com/MatejMa2ur/tikontrol/issues). If a change moves a feature to a new state, update its Status cell in [FEATURES.md](docs/FEATURES.md) and run `src/scripts/progress.py`.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- LICENSE -->
## License

No license has been chosen yet.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- TRADEMARKS -->
## Trademarks

tikontrol is an independent project. It is not affiliated with, endorsed by or sponsored by Ubiquiti Inc. or MikroTik.

UniFi is a trademark of Ubiquiti Inc. MikroTik, RouterOS and WinBox are trademarks of their respective owner. These names are used only to describe what tikontrol works with and what inspired it. tikontrol is built from public documentation and does not include Ubiquiti or MikroTik code, logos or product images.

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- CONTACT -->
## Contact

Matej Mazúr - [github.com/MatejMa2ur](https://github.com/MatejMa2ur)

Project Link: [https://github.com/MatejMa2ur/tikontrol](https://github.com/MatejMa2ur/tikontrol)

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- ACKNOWLEDGMENTS -->
## Acknowledgments

- [nasnet-panel](https://github.com/nasnet-community/nasnet-panel)
- [uPlot](https://github.com/leeoniya/uPlot)
- [Best-README-Template](https://github.com/othneildrew/Best-README-Template)

<p align="right">(<a href="#readme-top">back to top</a>)</p>

<!-- MARKDOWN LINKS & IMAGES -->
[contributors-shield]: https://img.shields.io/github/contributors/MatejMa2ur/tikontrol.svg?style=for-the-badge
[contributors-url]: https://github.com/MatejMa2ur/tikontrol/graphs/contributors
[forks-shield]: https://img.shields.io/github/forks/MatejMa2ur/tikontrol.svg?style=for-the-badge
[forks-url]: https://github.com/MatejMa2ur/tikontrol/network/members
[stars-shield]: https://img.shields.io/github/stars/MatejMa2ur/tikontrol.svg?style=for-the-badge
[stars-url]: https://github.com/MatejMa2ur/tikontrol/stargazers
[issues-shield]: https://img.shields.io/github/issues/MatejMa2ur/tikontrol.svg?style=for-the-badge
[issues-url]: https://github.com/MatejMa2ur/tikontrol/issues
[Svelte]: https://img.shields.io/badge/Svelte-FF3E00?style=for-the-badge&logo=svelte&logoColor=white
[Svelte-url]: https://svelte.dev/
[TypeScript]: https://img.shields.io/badge/TypeScript-3178C6?style=for-the-badge&logo=typescript&logoColor=white
[TypeScript-url]: https://www.typescriptlang.org/
[Vite]: https://img.shields.io/badge/Vite-646CFF?style=for-the-badge&logo=vite&logoColor=white
[Vite-url]: https://vite.dev/
