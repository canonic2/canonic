/** Browser HTTP adapter shared by application composition and review delivery. */
export async function requestJson(url, body) {
  const response = await fetch(
    url,
    body === undefined
      ? undefined
      : {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        },
  );
  const value = await response.json();
  if (!response.ok || value.ok === false)
    throw new Error(value.error || 'Workbench request failed.');
  return value;
}
