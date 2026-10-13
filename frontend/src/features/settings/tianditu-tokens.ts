export function parseTiandituBackups(text: string, primary = ''): string[] {
  const seen = new Set([primary.trim()].filter(Boolean))
  return text.split(/[\s,;，；]+/).map(token => token.trim()).filter(token => {
    if (!token || seen.has(token)) return false
    seen.add(token)
    return true
  })
}

export function primaryTiandituToken(settings?: {
  tianditu_token?: string | null
  tianditu_backup_tokens?: string[]
} | null): string | null {
  return settings?.tianditu_token?.trim()
    || settings?.tianditu_backup_tokens?.find(token => token.trim())?.trim()
    || null
}
