import { getCollection } from 'astro:content';
import { docTitle } from '../../../lib/docs.js';

export async function GET() {
  const entries = await getCollection('workbenchDocs');
  return new Response(JSON.stringify(entries.map(entry => ({
    id: entry.id,
    text: `${docTitle(entry)} ${entry.body}`.toLowerCase(),
  }))), { headers: { 'Content-Type': 'application/json' } });
}
