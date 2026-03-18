import { createOpencodeClient, type Session, type TextPart } from "@opencode-ai/sdk"

const port = process.argv[2] || "4096"
const client = createOpencodeClient({ baseUrl: `http://localhost:${port}` })

console.log(`Connecting to opencode on port ${port}...`)

// List sessions
const sessionsResult = await client.session.list()
if (sessionsResult.error || !sessionsResult.data) {
  console.error("Failed to list sessions. Is opencode running on this port?", sessionsResult.error)
  process.exit(1)
}

const sessions = sessionsResult.data
if (sessions.length === 0) {
  console.log("No sessions found. Create one in the opencode TUI first.")
  process.exit(1)
}

console.log(`Found ${sessions.length} sessions:`)
sessions.forEach((s: Session, i: number) => console.log(`  [${i}] ${s.id}: "${s.title}"`))

const session = sessions[0]!
console.log(`\nUsing session: "${session.title}" (${session.id})`)

// Get available providers
console.log("\nFetching available providers...")
try {
  const providersResult = await client.provider.list()
  console.log("Providers:", JSON.stringify(providersResult.data, null, 2))
} catch (e) {
  console.log("Could not fetch providers:", e)
}

// Get messages to see conversation state
console.log("\nFetching messages...")
const messagesResult = await client.session.messages({ path: { id: session.id } })
if (messagesResult.error || !messagesResult.data) {
  console.log("Could not fetch messages:", messagesResult.error)
} else {
  const messages = messagesResult.data
  console.log(`Found ${messages.length} messages`)

  if (messages.length > 0) {
    const last = messages[messages.length - 1]!
    console.log(`Last message role: ${last.info.role}`)
    const textParts = last.parts.filter((p): p is TextPart => p.type === "text")
    if (textParts.length > 0) {
      console.log(`Last message preview: "${textParts[0]!.text.substring(0, 100)}..."`)
    }
  }
}

console.log("\n--- Spike complete ---")
console.log("To test sending a message, uncomment the chat section below and fill in providerID/modelID")
console.log("based on the providers output above.")

// Uncomment to test sending a message:
// const result = await client.session.prompt({
//   path: { id: session.id },
//   body: {
//     model: { providerID: "anthropic", modelID: "claude-sonnet-4-20250514" },
//     parts: [{ type: "text", text: "What files are in the current directory?" }],
//   },
// })
// console.log("Response:", JSON.stringify(result.data, null, 2))
