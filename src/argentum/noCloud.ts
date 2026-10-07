/**
 * RapidRAW Cloud, switched off. Ours.
 *
 * RapidRAW 1.6.5 sells remote generative edits through an account with
 * RapidRAW: a Clerk sign-in, a subscription, and requests to getrapidraw.com.
 * Argentum has no account, takes no money in another author's name, and does
 * not contact a server it was never asked to.
 *
 * Their `AppWrapper` signs in to Clerk on every launch, before anyone has
 * chosen Cloud - it registers the app with Clerk's servers and loads the
 * sign-in screens from the web. Their own store already knows a platform where
 * Cloud cannot run: it marks itself 'unsupported' on Android and iOS, and
 * `initAuth` then returns before touching the network. This says the same for
 * every platform, at import, which is before any effect of theirs can run.
 *
 * The Cloud tile is hidden from our side (NoCloudTile.tsx). The rest is in their
 * build files: the Clerk plugin and its permissions are not built in, and the
 * HTTP permission allows no address. See the `no-cloud` entry in
 * scripts/upstream-registry.mjs.
 */

import { useCloudStore } from '../store/useCloudStore';

useCloudStore.setState({ authStatus: 'unsupported' });
