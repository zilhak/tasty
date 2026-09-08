/**
 * The path the site is served from.
 *
 * This is the single place the deployment path is written. Everything else
 * derives from it:
 *
 *   - `astro.config.mjs` passes it as Astro's `base`, which puts it in
 *     `import.meta.env.BASE_URL`, which `src/lib/site.ts` `url()` joins onto
 *     every internal link — and which the dev server mounts the site under, so
 *     local and deployed resolve the same way.
 *   - the markdown pipeline gets it directly, because it runs from the Astro
 *     config and so outside the environment that carries `BASE_URL`.
 *   - `scripts/check-links.mjs` gets it directly, so it verifies the links the
 *     deployed site will actually serve.
 *
 * The repository publishes as a GitHub project page, which serves from the
 * repository name rather than the domain root. A custom domain would serve from
 * the root: set this to "" and nothing else changes.
 */
export const BASE = "/tasty";
