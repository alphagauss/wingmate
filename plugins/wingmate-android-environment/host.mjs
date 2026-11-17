export const name = 'wingmate-android-environment';
export const inject = ['systemPrompt'];

export function apply(ctx) {
  ctx.systemPrompt.section({
    name: 'wingmate:android-environment',
    order: ctx.systemPrompt.getSectionOrder('DEPLOYMENT_PERSONA_SUFFIX') + 1,
    interpolate: false,
    text: 'The local Wingmate agent runs in Ubuntu on Android through PRoot. Use Node.js fetch for HTTP requests, Python sqlite3 for SQLite databases, and Python zipfile for ZIP archives.',
  });
}
