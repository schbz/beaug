# Changelog

All notable changes to Beaug will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.7.0] - 2026-01-28

### 🎉 Initial Public Release

First public release of Beaug - Batch EVM Allocation Utility GUI.

### Added

- [x] Cross-platform GUI built with Rust and eframe/egui
- [x] Modern dark theme with retro-inspired aesthetic
- [x] Dashboard overview with network status and Ledger connection at a glance
- [x] Settings persistence saved locally in OS-appropriate config directory
- [x] Native Ledger support with direct HID communication, thread-safe mutex synchronization, and retry logic with exponential backoff for transient errors
- [x] HD wallet derivation to derive and manage hundreds of addresses from BIP44 paths
- [x] Derivation path options supporting Account-index, Address-index, and custom modes
- [x] Custom coin types to override SLIP-44 for non-standard derivations
- [x] Async balance scanning with batch checks and automatic empty address detection
- [x] Consecutive empty detection to stop scanning after configurable empty addresses
- [x] Multi-network support for Ethereum, Optimism, Base, Polygon, BNB Chain, Avalanche, Linea, Gnosis, Celo, Pulsechain, Ethereum Classic, and Sepolia
- [x] Custom network configuration to add custom RPC endpoints and chain configs
- [x] Gas price controls with adjustable speed multiplier (0.8x - 2.5x)
- [x] EIP-1559 support with automatic detection where supported
- [x] Legacy chain support for chains without EIP-1559
- [x] Real-time async transaction queue with visual status tracking
- [x] Split even to distribute funds evenly across multiple recipients
- [x] Split random to distribute funds in randomized amounts for privacy
- [x] Bulk disperse for gas-efficient batch transfers via smart contract (single signature)
- [x] Tip support for optional tip recipient in bulk disperse operations
- [x] Mixed distribution supporting both equal and custom amounts per recipient
- [x] Operation logs with persistent log of all operations and timestamps
- [x] Notification system with comprehensive error handling and user-friendly messages
- [x] Address export to CSV files
- [x] Backup cast mode with optional Foundry `cast` CLI fallback in settings

---

## [Unreleased]

_Future changes will be documented here before release._

<!-- Template for new entries:
## [X.X.X] - YYYY-MM-DD

### Added
- New features

### Changed
- Changes to existing functionality

### Fixed
- Bug fixes

### Removed
- Removed features
-->
