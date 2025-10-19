/**
 * A* pathfinding algorithm for navigation mesh
 * Adapted from javascript-astar by Brian Grinstead
 */

interface Vec2 {
  x: number;
  y: number;
}

interface AStarNode {
  id: string;
  pos: Vec2;
  f: number;
  g: number;
  h: number;
  visited: boolean;
  closed: boolean;
  parent: AStarNode | null;
}

interface PathNode {
  id: string;
  pos: Vec2;
}

interface NavGraph {
  getNode(id: string): PathNode | undefined;
  getNeighbors(nodeId: string): Array<{ node: PathNode; cost: number }>;
}

class BinaryHeap<T> {
  #content: T[] = [];
  #scoreFunction: (node: T) => number;

  constructor(scoreFunction: (node: T) => number) {
    this.#scoreFunction = scoreFunction;
  }

  push(element: T): void {
    this.#content.push(element);
    this.#sinkDown(this.#content.length - 1);
  }

  pop(): T | undefined {
    const result = this.#content[0];
    const end = this.#content.pop();
    if (this.#content.length > 0 && end !== undefined) {
      this.#content[0] = end;
      this.#bubbleUp(0);
    }
    return result;
  }

  size(): number {
    return this.#content.length;
  }

  rescoreElement(node: T): void {
    this.#sinkDown(this.#content.indexOf(node));
  }

  #sinkDown(n: number): void {
    const element = this.#content[n];
    while (n > 0) {
      const parentN = ((n + 1) >> 1) - 1;
      const parent = this.#content[parentN];
      if (this.#scoreFunction(element) < this.#scoreFunction(parent)) {
        this.#content[parentN] = element;
        this.#content[n] = parent;
        n = parentN;
      } else {
        break;
      }
    }
  }

  #bubbleUp(n: number): void {
    const length = this.#content.length;
    const element = this.#content[n];
    const elemScore = this.#scoreFunction(element);

    while (true) {
      const child2N = (n + 1) << 1;
      const child1N = child2N - 1;
      let swap: number | null = null;
      let child1Score: number = 0;

      if (child1N < length) {
        const child1 = this.#content[child1N];
        child1Score = this.#scoreFunction(child1);
        if (child1Score < elemScore) {
          swap = child1N;
        }
      }

      if (child2N < length) {
        const child2 = this.#content[child2N];
        const child2Score = this.#scoreFunction(child2);
        if (child2Score < (swap === null ? elemScore : child1Score)) {
          swap = child2N;
        }
      }

      if (swap !== null) {
        this.#content[n] = this.#content[swap];
        this.#content[swap] = element;
        n = swap;
      } else {
        break;
      }
    }
  }
}

/**
 * Heuristic functions for A*
 */
export const Heuristics = {
  /**
   * Manhattan distance (for grid-based movement)
   */
  manhattan(pos0: Vec2, pos1: Vec2): number {
    const d1 = Math.abs(pos1.x - pos0.x);
    const d2 = Math.abs(pos1.y - pos0.y);
    return d1 + d2;
  },

  /**
   * Euclidean distance (straight-line distance)
   */
  euclidean(pos0: Vec2, pos1: Vec2): number {
    const dx = pos1.x - pos0.x;
    const dy = pos1.y - pos0.y;
    return Math.sqrt(dx * dx + dy * dy);
  },

  /**
   * Diagonal distance (for 8-directional movement)
   */
  diagonal(pos0: Vec2, pos1: Vec2): number {
    const D = 1;
    const D2 = Math.sqrt(2);
    const d1 = Math.abs(pos1.x - pos0.x);
    const d2 = Math.abs(pos1.y - pos0.y);
    return D * (d1 + d2) + (D2 - 2 * D) * Math.min(d1, d2);
  },
};

export interface AStarOptions {
  /**
   * Return path to closest node if target is unreachable
   */
  closest?: boolean;
  /**
   * Heuristic function to estimate distance to goal
   */
  heuristic?: (pos0: Vec2, pos1: Vec2) => number;
}

/**
 * Find path from start to end using A* algorithm
 */
export function findPath(
  graph: NavGraph,
  startId: string,
  endId: string,
  options: AStarOptions = {}
): string[] {
  const heuristic = options.heuristic || Heuristics.euclidean;
  const closest = options.closest || false;

  const startNode = graph.getNode(startId);
  const endNode = graph.getNode(endId);

  if (!startNode || !endNode) {
    return [];
  }

  // Map of node IDs to A* search state
  const nodes = new Map<string, AStarNode>();

  const getAStarNode = (id: string, pos: Vec2): AStarNode => {
    let node = nodes.get(id);
    if (!node) {
      node = {
        id,
        pos,
        f: 0,
        g: 0,
        h: 0,
        visited: false,
        closed: false,
        parent: null,
      };
      nodes.set(id, node);
    }
    return node;
  };

  const openHeap = new BinaryHeap<AStarNode>((node) => node.f);
  const start = getAStarNode(startId, startNode.pos);
  const end = getAStarNode(endId, endNode.pos);
  let closestNode = start;

  start.h = heuristic(start.pos, end.pos);
  openHeap.push(start);

  while (openHeap.size() > 0) {
    const currentNode = openHeap.pop();
    if (!currentNode) break;

    // Found the goal
    if (currentNode.id === endId) {
      return reconstructPath(currentNode);
    }

    currentNode.closed = true;

    // Check all neighbors
    const neighbors = graph.getNeighbors(currentNode.id);
    for (const { node: neighborNode, cost } of neighbors) {
      const neighbor = getAStarNode(neighborNode.id, neighborNode.pos);

      if (neighbor.closed) {
        continue;
      }

      const gScore = currentNode.g + cost;
      const beenVisited = neighbor.visited;

      if (!beenVisited || gScore < neighbor.g) {
        // Found better path to this node
        neighbor.visited = true;
        neighbor.parent = currentNode;
        neighbor.h = neighbor.h || heuristic(neighbor.pos, end.pos);
        neighbor.g = gScore;
        neighbor.f = neighbor.g + neighbor.h;

        if (closest) {
          if (
            neighbor.h < closestNode.h ||
            (neighbor.h === closestNode.h && neighbor.g < closestNode.g)
          ) {
            closestNode = neighbor;
          }
        }

        if (!beenVisited) {
          openHeap.push(neighbor);
        } else {
          openHeap.rescoreElement(neighbor);
        }
      }
    }
  }

  // No path found
  if (closest) {
    return reconstructPath(closestNode);
  }

  return [];
}

/**
 * Reconstruct path from start to node by following parent pointers
 */
function reconstructPath(node: AStarNode): string[] {
  const path: string[] = [];
  let current: AStarNode | null = node;

  while (current) {
    path.unshift(current.id);
    current = current.parent;
  }

  return path;
}
