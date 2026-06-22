export type ParsedSemver = [number, number, number, string];

export function parseSemver(input: string): ParsedSemver | null {
  const match = input.match(
    /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/,
  );
  if (!match) {
    return null;
  }

  return [
    Number.parseInt(match[1], 10),
    Number.parseInt(match[2], 10),
    Number.parseInt(match[3], 10),
    match[4] ?? "",
  ];
}

function comparePrerelease(left: string, right: string): number {
  if (left === right) {
    return 0;
  }
  if (left === "") {
    return 1;
  }
  if (right === "") {
    return -1;
  }

  const leftParts = left.split(".");
  const rightParts = right.split(".");
  const max = Math.max(leftParts.length, rightParts.length);

  for (let index = 0; index < max; index += 1) {
    const leftPart = leftParts[index];
    const rightPart = rightParts[index];

    if (leftPart === undefined) {
      return -1;
    }
    if (rightPart === undefined) {
      return 1;
    }

    const leftNumber = /^\d+$/.test(leftPart)
      ? Number.parseInt(leftPart, 10)
      : null;
    const rightNumber = /^\d+$/.test(rightPart)
      ? Number.parseInt(rightPart, 10)
      : null;

    if (leftNumber !== null && rightNumber !== null) {
      if (leftNumber !== rightNumber) {
        return leftNumber > rightNumber ? 1 : -1;
      }
      continue;
    }
    if (leftNumber !== null) {
      return -1;
    }
    if (rightNumber !== null) {
      return 1;
    }
    if (leftPart !== rightPart) {
      return leftPart > rightPart ? 1 : -1;
    }
  }

  return 0;
}

export function compareSemver(left: string, right: string): number {
  const leftParsed = parseSemver(left);
  const rightParsed = parseSemver(right);

  if (!leftParsed || !rightParsed) {
    return left.localeCompare(right);
  }

  for (let index = 0; index < 3; index += 1) {
    if (leftParsed[index] !== rightParsed[index]) {
      return leftParsed[index] > rightParsed[index] ? 1 : -1;
    }
  }

  return comparePrerelease(leftParsed[3], rightParsed[3]);
}

export function newestByVersion<T extends { version: string }>(
  releases: T[],
): T | undefined {
  return [...releases].sort((left, right) =>
    compareSemver(right.version, left.version)
  )[0];
}
