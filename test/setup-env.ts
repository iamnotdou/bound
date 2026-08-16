// Runs before any test module is imported.
//
// As of step 3.3, app/lib/config.ts reads public configuration from the
// committed deployments map — it no longer touches process.env or .env.testnet.
// This file stays as the vitest setup hook for any future env seeding the unit
// suite might need; it intentionally sets nothing today.
