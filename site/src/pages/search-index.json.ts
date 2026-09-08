import type { APIRoute } from "astro";
import { buildPages } from "../lib/guide";
import { searchIndex } from "../lib/search";

export const GET: APIRoute = () => {
  const root = process.cwd();
  return new Response(JSON.stringify(searchIndex(root, buildPages(root), "en")), {
    headers: { "content-type": "application/json" },
  });
};
