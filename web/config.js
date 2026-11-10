// Edit this file after deploying the contract (see README).
export const TOKENS = {
  // Circle's USDC on NEAR testnet (6 decimals). Free test USDC: https://faucet.circle.com (pick NEAR Testnet).
  usdc: { account_id: "3e2210e1184b45b64c8a434c0a7e7b23cc04ea7eb7a6c3c32520d03d4afcb8af", symbol: "USDC", decimals: 6 },
  // Wrapped NEAR on testnet (24 decimals): wrap free faucet NEAR 1:1.
  wnear: { account_id: "wrap.testnet", symbol: "wNEAR", decimals: 24 },
};

export const CONFIG = {
  network: "testnet",
  // The account you deployed contract/ to, e.g. "lot-passport.yourname.testnet".
  contractId: "nauval-lp-contract.testnet",
  // Public RPC. If it rate-limits you, pick another from https://docs.near.org/api/rpc/providers
  rpc: "https://rpc.testnet.fastnear.com",
  explorer: "https://testnet.nearblocks.io",
  // Fallback only: the app reads the real payment token from the contract (payment_token view).
  token: TOKENS.usdc,
  // Optional gasless signing for suppliers and inspectors: run server/ and put its URL here.
  relayerUrl: "http://localhost:8787",
  intents: {
    // NEAR Intents has no testnet: the app only asks for a dry (preview) quote on mainnet.
    api: "https://1click.chaindefuser.com",
    // USDC on NEAR mainnet, the asset a passport would be paid in on mainnet.
    destinationAsset: "nep141:17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1",
  },
};
