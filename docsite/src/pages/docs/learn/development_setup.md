## Development Setup

### Local Testing

1. In your Anchor.toml, make sure you have a test keypair defined:

```toml
[provider]
cluster = "localnet"
wallet = "~/.config/solana/id.json"  # Your local keypair. Find address using `solana address`
```

2. Install dependencies:

```bash
yarn install
```

3. Run the tests:

```bash
env TESTING=true anchor test
```