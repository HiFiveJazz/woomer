#version 330

in vec2 fragTexCoord;
in vec4 fragColor;

uniform sampler2D texture0;
uniform vec4 colDiffuse;

uniform vec4 spotlightTint;
uniform vec2 cursorPosition;
uniform float spotlightRadiusMultiplier;

const float UNIT_RADIUS = 60.0;
const float EDGE_SOFTNESS = 60.0;

out vec4 finalColor;

void main()
{
    vec4 texelColor = texture(texture0, fragTexCoord);

    float distanceToCursor = distance(gl_FragCoord.xy, cursorPosition);
    float spotlightRadius = UNIT_RADIUS * spotlightRadiusMultiplier;

    float t = smoothstep(
        spotlightRadius - EDGE_SOFTNESS,
        spotlightRadius + EDGE_SOFTNESS,
        distanceToCursor
    );

    vec4 tinted = mix(texelColor, vec4(spotlightTint.rgb, 1.0), spotlightTint.a);

    finalColor = mix(texelColor, tinted, t) * colDiffuse;
}
