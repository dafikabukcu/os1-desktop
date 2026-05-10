export class InfinityLoader {
  constructor(
    canvas: HTMLCanvasElement,
    options?: {
      transitionDurationMs?: number;
      maxCanvasSize?: number;
    },
  );
  onComplete: (() => void) | null;
  start(): void;
  triggerTransition(): void;
  morphToDot(duration?: number): Promise<void>;
  fadeOut(duration?: number): Promise<void>;
  destroy(): void;
}
