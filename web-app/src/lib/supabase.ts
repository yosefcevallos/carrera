// Browser Supabase client. Only the publishable (anon) key is ever used here; RLS grants read on
// the public tables and views. Returns undefined when the env is absent so callers keep zero behaviour.
import { createClient, type SupabaseClient } from "@supabase/supabase-js";

const URL = process.env.NEXT_PUBLIC_SUPABASE_URL ?? "";
const KEY = process.env.NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY ?? "";

let client: SupabaseClient | undefined;
let warned = false;

export function getSupabase(): SupabaseClient | undefined {
  if (!URL || !KEY) {
    if (!warned) {
      warned = true;
      console.warn("[supabase] NEXT_PUBLIC_SUPABASE_URL / NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY not set; history stays zero.");
    }
    return undefined;
  }
  if (!client) client = createClient(URL, KEY, { auth: { persistSession: false, autoRefreshToken: false } });
  return client;
}
