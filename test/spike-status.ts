import { createOpencodeClient } from "@opencode-ai/sdk"

const port = process.argv[2] || "4096"
const client = createOpencodeClient({ baseUrl: `http://localhost:${port}` })

console.log(`Connecting to opencode on port ${port}...`)

const sessionsResult = await client.session.list()
if (sessionsResult.error || !sessionsResult.data) {
  console.error("Failed to list sessions. Is opencode running on this port?", sessionsResult.error)
  process.exit(1)
}

const sessions = sessionsResult.data
console.log(`Found ${sessions.length} sessions:`)
for (const s of sessions) {
  console.log(`  - ${s.id}: "${s.title}" (created: ${new Date(s.time.created).toLocaleString()})`)
}

console.log("\nListening for events (Ctrl+C to stop)...")
console.log("Interact with the opencode instance to see events flow.\n")

try {
  const { stream } = await client.event.subscribe()
  for await (const event of stream) {
    const timestamp = new Date().toLocaleTimeString()
    console.log(`[${timestamp}] ${event.type}`)
    console.log(`  ${JSON.stringify(event.properties, null, 2).split("\n").join("\n  ")}`)
    console.log()
  }
} catch (e) {
  console.error("Event stream error:", e)
}
